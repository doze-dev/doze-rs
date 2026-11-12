//! IAM as the authorizer: who a request is, which policies apply to them, and what the evaluator says. In soft mode a request that
//! would be refused is logged and goes through; in enforce mode it is refused in AWS's words.
//!
//! Who the caller is comes from the credentials alone: a session token this account signed is a role session; an access key IAM
//! issued is its user; anything else (`test`, a key from the command line) is the account root, which is allowed everything.
//! Soft mode never asks the actor for a key that IAM cannot have issued, so an account with no identities costs nothing.

use aws_iam_policy::{Context, Decision, Document, Layer, Policies, Request, Subject, Verdict};
use aws_peers::{Mode, Need};
use aws_wire::ApiError;
use aws_wire::service::{Call, Caller};

use crate::core::{Core, Which};
use crate::gather::Docs;
use crate::model::{Entity, Kind};
use crate::req::Req;
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
}

/// Who a request comes from.
#[derive(Debug)]
enum Resolved {
    /// The account root, or a key IAM did not issue: everything is allowed.
    Root,
    Principal(Box<Principal>),
}

impl Core {
    /// Whether `caller` may do all of `needs`. In soft mode the answer is always yes (a refusal is logged).
    pub(crate) fn authorize(&self, caller: &Caller, needs: &[Need]) -> Result<(), ApiError> {
        if self.mode == Mode::Off {
            return Ok(());
        }
        let principal = match self.resolve(caller)? {
            Resolved::Root => return Ok(()),
            Resolved::Principal(p) => p,
        };
        for need in needs {
            let (decision, resource_docs) = self.decide(&principal, caller, need);
            if decision.allowed() {
                continue;
            }
            let message = refusal(&principal, need, &decision);
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
            .set(
                "aws:SecureTransport",
                if caller.endpoint.starts_with("https") {
                    "true"
                } else {
                    "false"
                },
            )
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
        }
    }

    fn of_session(&self, s: &Session, caller: &Caller) -> Principal {
        let role = s.subject.rsplit('/').next().unwrap_or_default();
        let entity = self.entity(Kind::Role, role).ok();
        let (identity, boundary) = entity.map(|e| self.policies_of(e)).unwrap_or_default();
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
        }
    }
}

fn tags<'a>(c: &mut Context, pairs: impl Iterator<Item = (&'a String, &'a String)>) {
    for (k, v) in pairs {
        c.set(&format!("aws:PrincipalTag/{k}"), v.clone());
    }
}

/// AWS's sentence for a refusal: who, what, on what, and which layer said no.
fn refusal(p: &Principal, need: &Need, d: &Decision) -> String {
    let who = &p.shown;
    let action = &need.action;
    let head = format!(
        "User: {who} is not authorized to perform: {action} on resource: {} ",
        need.resource
    );
    let why = if d.verdict == Verdict::ExplicitDeny {
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
    };
    format!("{head}{why}")
}

impl Core {
    /// Every thing a call to IAM or STS asks to do. `GetCallerIdentity` and `GetSessionToken` are never asked about: AWS lets any
    /// caller see who they are, and a session token is made from what a user already has.
    pub(crate) fn needs_of(&self, which: Which, call: &Call) -> Vec<Need> {
        let r = Req::new(call);
        match which {
            Which::Sts => self.sts_needs(call, &r),
            Which::Iam => {
                let Some(op) = aws_model_iam::Operation::from_index(call.op) else {
                    return Vec::new();
                };
                let mut need = Need::new(
                    format!("iam:{op:?}"),
                    self.iam_resource(op, &r, &call.caller),
                );
                if let Some(arn) = r.opt("PolicyArn") {
                    need = need.with("iam:PolicyARN", vec![arn.to_owned()]);
                }
                if let Some(arn) = r.opt("PermissionsBoundary") {
                    need = need.with("iam:PermissionsBoundary", vec![arn.to_owned()]);
                }
                vec![need]
            }
        }
    }

    fn sts_needs(&self, call: &Call, r: &Req<'_>) -> Vec<Need> {
        use aws_model_sts::Operation as O;
        let Some(op) = O::from_index(call.op) else {
            return Vec::new();
        };
        match op {
            O::GetCallerIdentity
            | O::GetSessionToken
            | O::AssumeRoleWithWebIdentity
            | O::AssumeRoleWithSAML => Vec::new(),
            O::AssumeRole => {
                let arn = r.s("RoleArn");
                let trust = arn
                    .rsplit('/')
                    .next()
                    .and_then(|name| self.entity(Kind::Role, name).ok())
                    .and_then(|role| role.trust.clone());
                vec![Need::new("sts:AssumeRole", arn).with_policy(trust).gated()]
            }
            other => vec![Need::new(format!("sts:{other:?}"), "*")],
        }
    }

    /// The ARN of what an IAM operation is on: the entity, policy or profile it names, else `*`.
    fn iam_resource(&self, op: aws_model_iam::Operation, r: &Req<'_>, caller: &Caller) -> String {
        use aws_model_iam::Operation as O;
        let account = &caller.account_id;
        let entity = |kind: Kind, name: &str| {
            self.entity(kind, name).map_or_else(
                |_| {
                    let path = r.opt("Path").unwrap_or("/");
                    format!("arn:aws:iam::{account}:{}{path}{name}", kind.word())
                },
                |e| e.arn.clone(),
            )
        };
        let group_first = matches!(op, O::AddUserToGroup | O::RemoveUserFromGroup);
        if group_first && let Some(name) = r.opt("GroupName") {
            return entity(Kind::Group, name);
        }
        if let Some(name) = r.opt("UserName") {
            return entity(Kind::User, name);
        }
        if let Some(name) = r.opt("GroupName") {
            return entity(Kind::Group, name);
        }
        if let Some(name) = r.opt("RoleName") {
            return entity(Kind::Role, name);
        }
        for member in ["PolicyArn", "OpenIDConnectProviderArn", "SAMLProviderArn"] {
            if let Some(arn) = r.opt(member) {
                return arn.to_owned();
            }
        }
        if let Some(name) = r.opt("InstanceProfileName") {
            let path = r.opt("Path").unwrap_or("/");
            return self.profiles.get(name).map_or_else(
                || format!("arn:aws:iam::{account}:instance-profile{path}{name}"),
                |p| p.arn.clone(),
            );
        }
        // A key made or listed without a user is the caller's own.
        if matches!(op, O::CreateAccessKey | O::ListAccessKeys)
            && let Ok(Resolved::Principal(p)) = self.resolve(caller)
            && let Some(arn) = p.subject.arns.first()
        {
            return arn.clone();
        }
        "*".to_owned()
    }
}
