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
mod certs;
mod core;
mod der;
mod details;
mod entity;
mod errors;
mod gather;
mod grants;
mod groups;
mod held;
mod heldops;
mod keys;
mod lastaccess;
mod local;
mod managed;
mod mfa;
mod model;
mod needs;
mod oidc;
mod ops;
mod password;
mod profiles;
mod providers;
mod report;
mod req;
mod saml;
mod samltrust;
mod session;
mod simulate;
mod sshkey;
mod storage;
mod sts;
mod totp;
mod tracked;
mod usercreds;
mod userheld;
mod view;
mod x509;
mod xmldsig;

/// Entry points for the fuzz targets (`crates/aws-iam/fuzz`): each reads any input and must not panic.
#[doc(hidden)]
pub mod fuzz {
    /// A server or signing certificate's body, a private key against it, and a chain.
    pub fn certificate(text: &str) {
        if let Some(cert) = crate::x509::certificate(text) {
            std::hint::black_box(crate::x509::key_matches(&cert, text));
        }
        std::hint::black_box(crate::x509::chain(text));
    }

    /// An SSH public key, in either encoding, written back in both.
    pub fn ssh_key(text: &str) {
        if let Ok(key) = crate::sshkey::read(text) {
            drop((key.fingerprint(), key.ssh(), key.pem()));
        }
    }
}

use std::sync::{Arc, Mutex};

use aws_peers::{Authorizer, Mode, Need, Peers};
use aws_schema::json::Violation;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Caller, Reply, Service, Web};
use doze_runtime::{Clock, Ctx, Lazy};

use crate::accesslog::{AccessLog, Seen};
use crate::core::{Answer, Ask, Core, Which};

/// The state both services share, behind one actor; and the access log, beside it.
#[derive(Debug)]
struct Shared {
    actor: Lazy<Core>,
    /// Where IAM keeps its file.
    dir: std::path::PathBuf,
    /// Kept outside the actor so it outlives the actor's idle sleeps. `None` until the first question.
    log: Mutex<Option<AccessLog>>,
    clock: Arc<dyn Clock>,
    peers: Option<Arc<Peers>>,
    /// The web identity providers' keys, for `AssumeRoleWithWebIdentity` (`oidc`).
    jwks: aws_webhook::jwks::Jwks,
}

impl Shared {
    /// Asks the actor, and holds the answer until what it changed is on disk.
    async fn ask(&self, which: Which, call: Call) -> Reply {
        if which == Which::Sts {
            oidc::verify(self, &call).await?;
        }
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
            self.tell_let_through(caller, &verdict, &seen);
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
                self.tell_let_through(caller, &verdict, &seen);
                self.record(seen);
                verdict
            }
            Ok(_) => Err(ApiError::internal(
                "IAM answered a question with something else",
            )),
            Err(e) => Err(ApiError::internal(e.to_string())),
        }
    }

    /// After a reset: the access log speaks of identities that are gone.
    fn forget_log(&self) {
        if let Ok(mut log) = self.log.lock() {
            *log = None;
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

    /// Soft mode answered yes to what the policies refused: the bus is told, for whoever marks the request (the console).
    fn tell_let_through(&self, caller: &Caller, verdict: &Result<(), ApiError>, seen: &[Seen]) {
        let Some(peers) = self.peers.as_ref() else {
            return;
        };
        if verdict.is_err() || peers.mode() != Mode::Soft {
            return;
        }
        for s in seen.iter().filter(|s| s.decision != "allowed") {
            peers.would_deny(caller, &s.action);
        }
    }

    fn mode(&self) -> Mode {
        self.peers.as_ref().map_or(Mode::default(), |p| p.mode())
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
        dir: ctx.dir.clone(),
        actor: Lazy::new("iam", ctx),
        log: Mutex::new(None),
        peers,
        jwks: oidc::keys(),
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
    fn storage(&self) -> Option<&dyn aws_wire::storage::Storage> {
        Some(self)
    }

    fn meta(&self) -> &'static aws_schema::meta::ServiceMeta {
        &aws_model_iam::meta::META
    }

    fn tables(&self) -> &'static aws_schema::tables::Tables {
        &aws_model_iam::tables::TABLES
    }

    fn serves(&self, op: u32) -> bool {
        aws_model_iam::Operation::from_index(op).is_some_and(|o| ops::BUILT.contains(&o))
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

    /// The model gives these two no auth scheme: the token or the assertion is the proof.
    fn unsigned(&self, op: u32) -> bool {
        use aws_model_sts::Operation as O;
        matches!(
            O::from_index(op),
            Some(O::AssumeRoleWithWebIdentity | O::AssumeRoleWithSAML)
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
