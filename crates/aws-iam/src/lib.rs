//! IAM and STS: the identities of an account (users, groups, roles, policies, keys, profiles, providers) and the sessions roles give
//! out. Two services on the wire (they speak the Query protocol, and have their own models and signing names) over one body of state
//! behind one lazy actor, kept on disk.
//!
//! IAM is also the authorizer every service asks (`--iam off|soft|enforce`): off asks nothing; soft evaluates every request against
//! the caller's policies, logs what would be refused and lets it through; enforce refuses it in AWS's words. Soft and enforce keep
//! an access log of the questions, from which doze generates a least-privilege policy (`accesslog`).

mod accesslog;
mod account;
mod anonymous;
mod attached;
mod authz;
mod awsmanaged;
mod core;
mod details;
mod entity;
mod errors;
mod gather;
mod groups;
mod keys;
mod managed;
mod model;
mod needs;
mod ops;
mod profiles;
mod providers;
mod req;
mod saml;
mod session;
mod simulate;
mod sts;
mod view;

use std::sync::{Arc, Mutex};

use aws_peers::{Authorizer, Mode, Need, Peers};
use aws_schema::json::Violation;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Caller, Reply, Service, Web, WebReply, WebRequest};
use doze_runtime::{Clock, Ctx, Lazy};
use serde_json::{Value, json};

use crate::accesslog::{AccessLog, Seen};
use crate::core::{Answer, Ask, Core, Which};

/// The state both services share, behind one actor; and the access log, beside it.
#[derive(Debug)]
struct Shared {
    actor: Lazy<Core>,
    /// Kept outside the actor so it outlives the actor's idle sleeps. `None` until the first question.
    log: Mutex<Option<AccessLog>>,
    clock: Arc<dyn Clock>,
    peers: Option<Arc<Peers>>,
}

impl Shared {
    /// Asks the actor, and holds the answer until what it changed is on disk.
    async fn ask(&self, which: Which, call: Call) -> Reply {
        let (mut reply, durable) = match self.actor.call(Ask::Call { which, call }).await {
            Ok(Answer::Call {
                reply,
                durable,
                seen,
            }) => {
                self.record(seen);
                (reply, durable)
            }
            Ok(_) => (
                Err(ApiError::internal(
                    "IAM answered a call with something else",
                )),
                None,
            ),
            Err(e) => (Err(ApiError::internal(e.to_string())), None),
        };
        if let Some(saved) = durable
            && let Err(e) = saved.await
        {
            reply = Err(ApiError::internal(format!("could not save: {e}")));
        }
        reply
    }

    /// Whether `caller` may do `needs`. A key IAM cannot have issued is the account root's, which is allowed everything: that is
    /// answered (and logged) here, without waking the actor, so an account with no identities costs nothing.
    async fn authorize(&self, caller: &Caller, needs: Vec<Need>) -> Result<(), ApiError> {
        // An unsigned request is nobody: only a resource policy that grants to anyone lets it in.
        if caller.is_anonymous() {
            let (verdict, seen) = anonymous::authorize(self.mode(), &needs, self.clock.now_ms());
            self.record(seen);
            return verdict;
        }
        if !(caller.access_key_id.starts_with("AKIA") || caller.access_key_id.starts_with("ASIA")) {
            let now = self.clock.now_ms();
            let root = format!("arn:aws:iam::{}:root", caller.account_id);
            self.record(needs.into_iter().map(|need| Seen {
                principal: root.clone(),
                role: None,
                action: need.action,
                resource: need.resource,
                decision: "allowed",
                reason: "the account root is allowed everything".into(),
                at_ms: now,
            }));
            return Ok(());
        }
        match self
            .actor
            .call(Ask::Authorize {
                caller: caller.clone(),
                needs,
            })
            .await
        {
            Ok(Answer::Authorize(verdict, seen)) => {
                self.record(seen);
                verdict
            }
            Ok(_) => Err(ApiError::internal(
                "IAM answered a question with something else",
            )),
            Err(e) => Err(ApiError::internal(e.to_string())),
        }
    }

    fn record(&self, seen: impl IntoIterator<Item = Seen>) {
        let mut seen = seen.into_iter().peekable();
        if seen.peek().is_none() {
            return;
        }
        if let Ok(mut log) = self.log.lock() {
            let log = log.get_or_insert_with(AccessLog::default);
            for s in seen {
                log.record(s);
            }
        }
    }

    fn mode(&self) -> Mode {
        self.peers.as_ref().map_or(Mode::default(), |p| p.mode())
    }

    /// `/_doze/iam/DozeAccessLog` and `/_doze/iam/DozeGeneratePolicy`, with their parameters in the query string or a form body.
    fn doze(&self, request: &WebRequest) -> WebReply {
        let mut form = aws_wire::query::parse_form(&request.query);
        if let Ok(body) = std::str::from_utf8(&request.body) {
            form.extend(aws_wire::query::parse_form(body));
        }
        self.doze_action(request.path.trim_start_matches(DOZE_PATH), &form)
    }

    /// A doze-only action (`DozeAccessLog`, `DozeGeneratePolicy`), by its name and parameters: from its own address, or as a Query
    /// `Action` on the IAM endpoint (as the Go edition served them).
    fn doze_action(
        &self,
        action: &str,
        form: &std::collections::BTreeMap<String, String>,
    ) -> WebReply {
        let yes = |k: &str| form.get(k).is_some_and(|v| v.eq_ignore_ascii_case("true"));
        let mode = match self.mode() {
            Mode::Off => "off",
            Mode::Soft => "soft",
            Mode::Enforce => "enforce",
        };
        let Ok(mut log) = self.log.lock() else {
            return web_reply(
                500,
                &json!({"Code": "InternalFailure", "Message": "The access log is unavailable."}),
            );
        };
        match action {
            "DozeAccessLog" => {
                if yes("Reset") {
                    *log = None;
                }
                let out = log.as_ref().map_or_else(
                    || json!({"Mode": mode, "Entries": [], "Evicted": 0}),
                    |l| l.entries(mode),
                );
                web_reply(200, &out)
            }
            "DozeGeneratePolicy" => {
                let principal = form.get("Principal").map(String::as_str);
                match log
                    .as_ref()
                    .and_then(|l| l.generate(principal, yes("ScopeToResources")))
                {
                    Some(out) => web_reply(200, &out),
                    None => {
                        let message = match (principal, mode) {
                            (_, "off") => "IAM is off, so nothing is recorded: run with --iam soft or enforce.".to_owned(),
                            (Some(p), _) => format!("No access has been recorded for principal {p}."),
                            (None, _) => "No access has been recorded yet.".to_owned(),
                        };
                        web_reply(404, &json!({"Code": "NoSuchEntity", "Message": message}))
                    }
                }
            }
            other => web_reply(
                404,
                &json!({"Code": "UnknownOperation", "Message": format!("doze has no IAM extension called {other:?}: there are DozeAccessLog and DozeGeneratePolicy.")}),
            ),
        }
    }
}

/// Where IAM's doze-only addresses live. `_` cannot begin a bucket name, so no S3 path-style address is taken.
const DOZE_PATH: &str = "/_doze/iam/";

fn web_reply(status: u16, body: &Value) -> WebReply {
    WebReply {
        status,
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: body.to_string().into(),
    }
}

impl Web for Iam {
    fn owns(&self, _host: &str, path: &str) -> bool {
        path.starts_with(DOZE_PATH)
    }

    fn serve(&self, request: WebRequest) -> BoxFuture<'_, WebReply> {
        Box::pin(async move { self.0.doze(&request) })
    }
}

impl Shared {
    async fn credential(
        &self,
        access_key_id: &str,
        session_token: Option<&str>,
    ) -> Option<aws_auth::Credential> {
        match self
            .actor
            .call(Ask::Credential {
                access_key_id: access_key_id.to_owned(),
                session_token: session_token.map(str::to_owned),
            })
            .await
        {
            Ok(Answer::Credential(found)) => found,
            _ => None,
        }
    }
}

impl aws_auth::CredentialSource for Iam {
    /// Only a key that looks issued by IAM (`AKIA` for an access key, `ASIA` for a session) wakes IAM.
    fn find<'a>(
        &'a self,
        access_key_id: &'a str,
        session_token: Option<&'a str>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Option<aws_auth::Credential>> + Send + 'a>,
    > {
        Box::pin(async move {
            if !(access_key_id.starts_with("AKIA") || access_key_id.starts_with("ASIA")) {
                return None;
            }
            self.0.credential(access_key_id, session_token).await
        })
    }
}

impl Authorizer for Iam {
    fn authorize<'a>(
        &'a self,
        caller: &'a Caller,
        needs: Vec<Need>,
    ) -> BoxFuture<'a, Result<(), ApiError>> {
        Box::pin(self.0.authorize(caller, needs))
    }
}

/// The IAM service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Iam(Arc<Shared>);

/// The STS service, over the same state as [`Iam`].
#[derive(Debug)]
pub struct Sts(Arc<Shared>);

/// IAM and STS over one body of state. `ctx` says where its files live and how it reads the time.
#[must_use]
pub fn services(ctx: Arc<Ctx>) -> (Iam, Sts) {
    let peers = ctx
        .ext
        .as_ref()
        .and_then(|e| Arc::clone(e).downcast::<Peers>().ok());
    let shared = Arc::new(Shared {
        clock: Arc::clone(&ctx.clock),
        actor: Lazy::new("iam", ctx),
        log: Mutex::new(None),
        peers,
    });
    (Iam(Arc::clone(&shared)), Sts(shared))
}

/// The words IAM and STS give a request that fails its model's checks: the Query services' `ValidationError`, and for a name that
/// breaks the rule, IAM's own sentence (recorded 2026-11-09).
fn invalid(tables: &'static aws_schema::tables::Tables, violation: &Violation) -> ApiError {
    if let Violation::Pattern {
        member,
        value: Some(_),
    } = violation
        && let Some(m) = tables.member(*member)
        && m.name.ends_with("Name")
        && !m.name.contains("Session")
    {
        let mut name = m.name.to_owned();
        if let Some(first) = name.get_mut(..1) {
            first.make_ascii_lowercase();
        }
        return ApiError::new(
            400,
            "ValidationError",
            format!(
                "The specified value for {name} is invalid. It must contain only alphanumeric characters and/or the following: +=,.@_-"
            ),
        );
    }
    let mut e = aws_wire::service::default_invalid(tables, violation);
    e.code = "ValidationError".into();
    e.shape = None;
    e
}

impl Service for Iam {
    fn meta(&self) -> &'static aws_schema::meta::ServiceMeta {
        &aws_model_iam::meta::META
    }

    fn tables(&self) -> &'static aws_schema::tables::Tables {
        &aws_model_iam::tables::TABLES
    }

    fn serves(&self, op: u32) -> bool {
        aws_model_iam::Operation::from_index(op).is_some_and(|o| ops::BUILT.contains(&o))
    }

    /// `Action=DozeAccessLog` and `Action=DozeGeneratePolicy` on the Query endpoint, as the Go edition served them (also at
    /// `/_doze/iam/<name>`). Signed like any IAM call.
    fn query_extension<'a>(
        &'a self,
        action: &'a str,
        form: &'a std::collections::BTreeMap<String, String>,
    ) -> Option<BoxFuture<'a, WebReply>> {
        action
            .starts_with("Doze")
            .then(|| -> BoxFuture<'a, WebReply> {
                Box::pin(async move { self.0.doze_action(action, form) })
            })
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(self.0.ask(Which::Iam, call))
    }

    fn web(&self) -> Option<&dyn Web> {
        Some(self)
    }

    fn invalid(&self, violation: &Violation) -> ApiError {
        invalid(self.tables(), violation)
    }
}

impl Service for Sts {
    fn meta(&self) -> &'static aws_schema::meta::ServiceMeta {
        &aws_model_sts::meta::META
    }

    fn tables(&self) -> &'static aws_schema::tables::Tables {
        &aws_model_sts::tables::TABLES
    }

    fn serves(&self, op: u32) -> bool {
        use aws_model_sts::Operation as O;
        matches!(
            O::from_index(op),
            Some(
                O::GetCallerIdentity
                    | O::AssumeRole
                    | O::AssumeRoleWithWebIdentity
                    | O::AssumeRoleWithSAML
                    | O::GetSessionToken
                    | O::GetFederationToken
                    | O::GetAccessKeyInfo
                    | O::DecodeAuthorizationMessage
                    | O::AssumeRoot
            )
        )
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(self.0.ask(Which::Sts, call))
    }

    fn invalid(&self, violation: &Violation) -> ApiError {
        invalid(self.tables(), violation)
    }
}

#[cfg(test)]
mod tests;
