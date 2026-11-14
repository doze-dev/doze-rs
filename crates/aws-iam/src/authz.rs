//! IAM as the authorizer: who a request is, which policies apply to them, and what the evaluator says. In soft mode a request that
//! would be refused is logged and goes through; in enforce mode it is refused in AWS's words.
//!
//! Who the caller is comes from the credentials alone: a session token this account signed is a role session; an access key IAM
//! issued is its user; anything else (`test`, a key from the command line) is the account root, which is allowed everything.
//! Soft mode never asks the actor for a key that IAM cannot have issued, so an account with no identities costs nothing.

use aws_iam_policy::{Context, Decision, Document, Layer, Policies, Request, Subject, Verdict};
use aws_peers::{Mode, Need};
use aws_wire::ApiError;
use aws_wire::service::Caller;

use crate::accesslog::Seen;
use crate::core::Core;
use crate::gather::Docs;
use crate::model::{Entity, Kind};
use crate::session::{Session, SessionKind, TokenFault};

/// Context keys AWS supplies on a real request and this emulator cannot: a condition that hangs on one is a guess, and the log says so.
const UNSUPPLIED: &[&str] = &[
    "aws:SourceIp",
    "aws:SourceVpc",
    "aws:SourceVpce",
    "aws:PrincipalOrgID",
    "aws:PrincipalOrgPaths",
    "aws:UserAgent",
    "aws:MultiFactorAuthPresent",
    "aws:MultiFactorAuthAge",
    "aws:TokenIssueTime",
    "aws:ResourceOrgID",
    "aws:ResourceAccount",
];

/// How often the last use of a credential is written: at most once a minute for each key or role. AWS itself says its last-used
/// data may lag by hours, so a minute is closer than anyone can tell, and a busy key does not write on every request.
const LAST_USED_EVERY_MS: i64 = 60_000;

/// A caller, resolved.
#[derive(Debug)]
struct Principal {
    /// How AWS words them in a refusal.
    shown: String,
    subject: Subject,
    identity: Docs,
    boundary: Docs,
    session: Option<Docs>,
    context: Context,
    /// The credential whose last use is noted.
    used: Used,
}

/// What a request was made with, for `GetAccessKeyLastUsed` and `RoleLastUsed`.
#[derive(Debug)]
enum Used {
    /// An access key IAM issued.
    Key(String),
    /// A session of a role this account has.
    Role(String),
    /// Anything else (a session for a role IAM does not hold, a user's own session token).
    Nothing,
}

/// Who a request comes from.
#[derive(Debug)]
enum Resolved {
    /// The account root, or a key IAM did not issue: everything is allowed.
    Root,
    Principal(Box<Principal>),
}

impl Core {
    /// Whether `caller` may do all of `needs`. In soft mode the answer is always yes (a refusal is logged). Every question is kept
    /// in [`Core::seen`] for the access log, and the credential's last use is noted.
    pub(crate) fn authorize(&mut self, caller: &Caller, needs: &[Need]) -> Result<(), ApiError> {
        if self.mode == Mode::Off {
            return Ok(());
        }
        let principal = match self.resolve(caller)? {
            Resolved::Root => {
                let now = self.now_ms();
                let root = format!("arn:aws:iam::{}:root", caller.account_id);
                self.seen.extend(needs.iter().map(|need| Seen {
                    principal: root.clone(),
                    role: None,
                    action: need.action.clone(),
                    resource: need.resource.clone(),
                    decision: "allowed",
                    reason: "the account root is allowed everything".into(),
                    at_ms: now,
                }));
                return Ok(());
            }
            Resolved::Principal(p) => p,
        };
        self.note_use(&principal.used, caller, needs);
        for need in needs {
            let (decision, resource_docs) = self.decide(&principal, caller, need);
            let why = (!decision.allowed()).then(|| why_refused(&principal, need, &decision));
            self.seen.push(Seen {
                principal: principal.shown.clone(),
                role: principal.subject.arns.get(1).cloned(),
                action: need.action.clone(),
                resource: need.resource.clone(),
                decision: match decision.verdict {
                    Verdict::Allow => "allowed",
                    Verdict::ExplicitDeny => "explicitDeny",
                    Verdict::ImplicitDeny => "implicitDeny",
                },
                reason: why
                    .clone()
                    .unwrap_or_else(|| allowed_by(&principal, &decision)),
                at_ms: self.now_ms(),
            });
            let Some(why) = why else {
                continue;
            };
            let message = format!(
                "User: {} is not authorized to perform: {} on resource: {} {why}",
                principal.shown, need.action, need.resource
            );
            if self.mode == Mode::Enforce {
                return Err(ApiError::new(400, "AccessDeniedException", message));
            }
            tracing::warn!(
                action = %need.action,
                resource = %need.resource,
                unreliable = !decision.unsupplied.is_empty(),
                policies = resource_docs,
                "iam soft mode: this request would be refused: {message}"
            );
        }
        Ok(())
    }

    fn decide(&self, p: &Principal, caller: &Caller, need: &Need) -> (Decision, usize) {
        let mut context = p.context.clone();
        for (key, values) in &need.context {
            context.set_many(key, values.clone());
        }
        let resource: Option<Vec<Document>> = need
            .policy
            .as_deref()
            .and_then(aws_iam_policy::parse)
            .and_then(Result::ok)
            .map(|d| vec![d]);
        let resource_refs: Option<Vec<&Document>> = resource.as_ref().map(|v| v.iter().collect());
        let identity = p.identity.refs();
        let boundary = (!p.boundary.is_empty()).then(|| p.boundary.refs());
        let session = p.session.as_ref().map(Docs::refs);
        let policies = Policies {
            identity: &identity,
            boundary: boundary.as_deref(),
            session: session.as_deref(),
            resource: resource_refs.as_deref(),
            resource_gate: need.gate,
        };
        let subject = Subject {
            service: caller.service.clone(),
            ..p.subject.clone()
        };
        let request = Request {
            action: &need.action,
            resource: &need.resource,
            context: &context,
            subject: &subject,
        };
        (
            aws_iam_policy::evaluate(&policies, &request),
            identity.len(),
        )
    }

    fn resolve(&self, caller: &Caller) -> Result<Resolved, ApiError> {
        let key = self.account().signing_key;
        if caller.access_key_id.starts_with("ASIA") {
            let Some(token) = caller.session_token.as_deref() else {
                return Ok(Resolved::Root);
            };
            return match Session::verify(token, &key, self.now_ms()) {
                Ok(s) => Ok(Resolved::Principal(Box::new(self.of_session(&s, caller)))),
                Err(TokenFault::Expired) => Err(ApiError::new(
                    400,
                    "ExpiredToken",
                    "The security token included in the request is expired",
                )),
                Err(TokenFault::Invalid) => Err(ApiError::new(
                    403,
                    "InvalidClientTokenId",
                    "The security token included in the request is invalid.",
                )),
            };
        }
        if let Some(k) = self.keys.get(&caller.access_key_id)
            && let Ok(user) = self.entity(Kind::User, &k.user)
        {
            return Ok(Resolved::Principal(Box::new(self.of_user(user, caller))));
        }
        Ok(Resolved::Root)
    }

    fn base_context(&self, caller: &Caller, mut c: Context) -> Context {
        let now = self.now_ms() / 1000;
        c.set("aws:CurrentTime", aws_core::time::format_iso8601(now))
            .set("aws:EpochTime", now.to_string())
            .set("aws:RequestedRegion", caller.region.clone())
            .set("aws:PrincipalAccount", caller.account_id.clone())
            // Always true. The local hop is plain HTTP, but it stands in for AWS's endpoint, which the same SDK reaches over TLS:
            // reporting the local transport would make the usual deny-unless-TLS policy (CDK's `enforceSSL`, the standard S3 and
            // SQS hardening) refuse every request, where on AWS it never fires. The Go edition did the same.
            .set("aws:SecureTransport", "true")
            .set(
                "aws:ViaAWSService",
                if caller.service.is_some() {
                    "true"
                } else {
                    "false"
                },
            );
        for key in UNSUPPLIED {
            c.leave_unsupplied(key);
        }
        c
    }

    fn of_user(&self, user: &Entity, caller: &Caller) -> Principal {
        let (identity, boundary) = self.policies_of(user);
        let mut c = Context::new();
        c.set("aws:username", user.name.clone())
            .set("aws:userid", user.id.clone())
            .set("aws:PrincipalArn", user.arn.clone())
            .set("aws:PrincipalType", "User");
        tags(&mut c, user.tags.iter());
        Principal {
            shown: user.arn.clone(),
            subject: Subject {
                arns: vec![user.arn.clone()],
                account: caller.account_id.clone(),
                service: None,
            },
            identity,
            boundary,
            session: None,
            context: self.base_context(caller, c),
            used: Used::Key(caller.access_key_id.clone()),
        }
    }

    fn of_session(&self, s: &Session, caller: &Caller) -> Principal {
        let role = s.subject.rsplit('/').next().unwrap_or_default();
        let entity = self.entity(Kind::Role, role).ok();
        let (identity, boundary) = entity.map(|e| self.policies_of(e)).unwrap_or_default();
        let used = match entity {
            Some(e) if s.kind == SessionKind::Role => Used::Role(e.name.clone()),
            _ => Used::Nothing,
        };
        let arn = format!("arn:aws:sts::{}:assumed-role/{role}/{}", s.account, s.name);
        let mut c = Context::new();
        c.set("aws:userid", s.id.clone())
            .set("aws:PrincipalArn", arn.clone())
            .set("aws:PrincipalType", "AssumedRole");
        if s.kind == SessionKind::Role {
            c.set("aws:RoleSessionName", s.name.clone());
        }
        if let Some(e) = entity {
            tags(&mut c, e.tags.iter());
        }
        // Tags passed when the session was made win over the role's own.
        tags(&mut c, s.tags.iter().map(|(k, v)| (k, v)));
        let session = s.policy.as_deref().map(|text| {
            let mut docs = Docs::default();
            if let Some(Ok(doc)) = aws_iam_policy::parse(text) {
                docs.push(doc, "session".into(), "none");
            }
            docs
        });
        Principal {
            shown: arn.clone(),
            subject: Subject {
                arns: vec![arn, s.subject.clone()],
                account: s.account.clone(),
                service: None,
            },
            identity,
            boundary,
            session,
            context: self.base_context(caller, c),
            used,
        }
    }
}

fn tags<'a>(c: &mut Context, pairs: impl Iterator<Item = (&'a String, &'a String)>) {
    for (k, v) in pairs {
        c.set(&format!("aws:PrincipalTag/{k}"), v.clone());
    }
}

/// The end of AWS's sentence for a refusal (after who, what and on what): which layer said no.
fn why_refused(p: &Principal, need: &Need, d: &Decision) -> String {
    let action = &need.action;
    if d.verdict == Verdict::ExplicitDeny {
        let layer = match d.denied_by() {
            Some(Layer::Boundary) => "a permissions boundary",
            Some(Layer::Session) => "a session policy",
            Some(Layer::Resource) => "a resource-based policy",
            _ => "an identity-based policy",
        };
        format!("with an explicit deny in {layer}")
    } else {
        let allowed = |l: Layer| {
            d.matched
                .iter()
                .any(|m| m.layer == l && m.effect == aws_iam_policy::Effect::Allow)
        };
        let missing = if !allowed(Layer::Identity) {
            "identity-based policy"
        } else if need.gate && !allowed(Layer::Resource) {
            "resource-based policy"
        } else if !p.boundary.is_empty() && !allowed(Layer::Boundary) {
            "permissions boundary"
        } else {
            "session policy"
        };
        format!("because no {missing} allows the {action} action")
    }
}

/// For the access log: the statement that let a request through.
fn allowed_by(p: &Principal, d: &Decision) -> String {
    let Some(m) = d
        .matched
        .iter()
        .find(|m| m.effect == aws_iam_policy::Effect::Allow)
    else {
        return "allowed".into();
    };
    let source = match m.layer {
        Layer::Identity => p.identity.sources.get(m.document),
        Layer::Boundary => p.boundary.sources.get(m.document),
        Layer::Session => p.session.as_ref().and_then(|s| s.sources.get(m.document)),
        Layer::Resource => None,
    };
    let statement = m
        .sid
        .clone()
        .unwrap_or_else(|| format!("statement {}", m.statement + 1));
    match source {
        Some(s) => format!("allowed by {statement} of {} ({})", s.id, s.kind),
        None => format!("allowed by {statement} of the resource-based policy"),
    }
}

impl Core {
    /// Notes that the credential behind a request was used: at most once a minute for each, so a busy key costs no writes.
    fn note_use(&mut self, used: &Used, caller: &Caller, needs: &[Need]) {
        let now = self.now_ms();
        let fresh = |at: i64| now - at < LAST_USED_EVERY_MS;
        // The service is the one the request was for, as the action names it (`sqs` for `sqs:SendMessage`).
        let service = needs
            .first()
            .and_then(|n| n.action.split_once(':'))
            .map_or("N/A", |(s, _)| s);
        match used {
            Used::Key(id) => {
                if self
                    .keys
                    .get(id)
                    .is_some_and(|k| k.last_used.as_ref().is_some_and(|(at, _, _)| fresh(*at)))
                {
                    return;
                }
                if let Some(k) = self.keys.get_mut(id) {
                    // (memory) AWS names the service by its lower-case namespace; SNS's actions are spelled `SNS:...`.
                    k.last_used = Some((now, service.to_ascii_lowercase(), caller.region.clone()));
                }
            }
            Used::Role(name) => {
                if self
                    .entity(Kind::Role, name)
                    .is_ok_and(|e| e.last_used.as_ref().is_some_and(|(at, _)| fresh(*at)))
                {
                    return;
                }
                if let Ok(e) = self.entity_mut(Kind::Role, name) {
                    e.last_used = Some((now, caller.region.clone()));
                }
            }
            Used::Nothing => {}
        }
    }

    /// The ARN of the IAM identity behind `caller` (a user's, or a role session's), when it is one.
    pub(crate) fn caller_arn(&self, caller: &Caller) -> Option<String> {
        match self.resolve(caller) {
            Ok(Resolved::Principal(p)) => p.subject.arns.into_iter().next(),
            _ => None,
        }
    }
}
