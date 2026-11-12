//! STS: who a request is, and the sessions that give someone else's identity for a while.
//!
//! In **soft** mode (the only mode so far) a role can be assumed by anyone and a role that does not exist can be assumed too: a
//! session is made for its ARN. Real accounts refuse both, but a deploy tool's first act is to assume roles (CDK's bootstrap roles)
//! that a local account has never been told about. The trust policy is stored and returned, and is evaluated when enforcement is.

use aws_model_sts::Operation;
use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::entity::new_id;
use crate::model::Kind;
use crate::req::{Req, secs};
use crate::session::{Session, SessionKind};

const MIN_SECS: i64 = 900;
const MAX_ROLE_SECS: i64 = 43_200;
const DEFAULT_ROLE_SECS: i64 = 3600;
const DEFAULT_USER_SECS: i64 = 43_200;
const MAX_USER_SECS: i64 = 129_600;

/// Whom a session is for, and for how long.
struct Who<'a> {
    kind: SessionKind,
    subject: &'a str,
    name: &'a str,
    id: String,
    secs: i64,
}

/// Who a request is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Identity {
    pub(crate) arn: String,
    pub(crate) user_id: String,
    pub(crate) account: String,
}

fn validation(message: impl Into<String>) -> ApiError {
    ApiError::new(400, "ValidationError", message)
}

impl Core {
    /// The identity behind a call: a session if it carried a good token, an IAM user if its key is one, else the account root.
    pub(crate) fn identify(&self, r: &Req<'_>) -> Identity {
        let account = r.account().to_owned();
        let key = self.account().signing_key;
        if let Some(token) = r.call.caller.session_token.as_deref()
            && let Ok(s) = Session::verify(token, &key, self.now_ms())
        {
            return session_identity(&s);
        }
        if let Some(k) = self.keys.get(&r.call.caller.access_key_id)
            && let Ok(u) = self.entity(Kind::User, &k.user)
        {
            return Identity {
                arn: u.arn.clone(),
                user_id: u.id.clone(),
                account,
            };
        }
        Identity {
            arn: format!("arn:aws:iam::{account}:root"),
            user_id: account.clone(),
            account,
        }
    }

    pub(crate) fn run_sts(&mut self, op: u32, r: &Req<'_>) -> Result<Value, ApiError> {
        match Operation::from_index(op) {
            Some(Operation::GetCallerIdentity) => {
                let i = self.identify(r);
                Ok(json!({"UserId": i.user_id, "Account": i.account, "Arn": i.arn}))
            }
            Some(Operation::AssumeRole) => self.assume_role(r),
            Some(Operation::AssumeRoleWithWebIdentity) => {
                self.assume_with_token(r, "WebIdentityToken")
            }
            Some(Operation::AssumeRoleWithSAML) => self.assume_with_token(r, "SAMLAssertion"),
            Some(Operation::GetSessionToken) => self.get_session_token(r),
            Some(Operation::GetFederationToken) => self.get_federation_token(r),
            Some(Operation::GetAccessKeyInfo) => self.get_access_key_info(r),
            Some(Operation::DecodeAuthorizationMessage) => Err(ApiError::modeled(
                "InvalidAuthorizationMessageException",
                "The message is invalid: doze-aws has not written an authorization failure message that could be decoded.",
            )),
            _ => Err(ApiError::new(
                501,
                "NotImplemented",
                "This operation is not built in doze-aws.",
            )),
        }
    }

    /// The credentials block for a session.
    fn credentials(&self, s: &Session) -> Value {
        let key = self.account().signing_key;
        json!({
            "AccessKeyId": s.access_key_id(&key), "SecretAccessKey": s.secret_access_key(&key),
            "SessionToken": s.token(&key), "Expiration": secs(s.expires_ms)
        })
    }

    fn session(&self, who: Who<'_>, r: &Req<'_>) -> Result<Session, ApiError> {
        Ok(Session {
            kind: who.kind,
            account: r.account().to_owned(),
            subject: who.subject.to_owned(),
            name: who.name.to_owned(),
            id: who.id,
            expires_ms: self.now_ms() + who.secs * 1000,
            policy: r.opt("Policy").map(str::to_owned),
            tags: r.tags(),
            source_identity: r.opt("SourceIdentity").map(str::to_owned),
            nonce: new_id("")?,
        })
    }

    fn assume_role(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let arn = r.s("RoleArn");
        let name = role_name_of(arn)?;
        let known = self.entity(Kind::Role, name).ok();
        let max = known.map_or(MAX_ROLE_SECS, |e| e.max_session_secs);
        let duration = r
            .int("DurationSeconds")
            .unwrap_or(DEFAULT_ROLE_SECS.min(max));
        if duration < MIN_SECS {
            return Err(validation(format!(
                "1 validation error detected: Value '{duration}' at 'durationSeconds' failed to satisfy constraint: Member must have value greater than or equal to 900"
            )));
        }
        if duration > max {
            return Err(validation(
                "The requested DurationSeconds exceeds the MaxSessionDuration set for this role.",
            ));
        }
        let role_id = known.map_or_else(|| new_id("AROA"), |e| Ok(e.id.clone()))?;
        let session_name = r.s("RoleSessionName");
        let s = self.session(
            Who {
                kind: SessionKind::Role,
                subject: arn,
                name: session_name,
                id: format!("{role_id}:{session_name}"),
                secs: duration,
            },
            r,
        )?;
        Ok(json!({
            "Credentials": self.credentials(&s),
            "AssumedRoleUser": {"AssumedRoleId": s.id, "Arn": format!("arn:aws:sts::{}:assumed-role/{name}/{session_name}", r.account())},
            "SourceIdentity": s.source_identity,
        }))
    }

    /// `AssumeRoleWithWebIdentity` and `AssumeRoleWithSAML`: the token or assertion is not checked (signatures are not validated); the
    /// subject, issuer and audience are read from it when it is a JWT.
    fn assume_with_token(&self, r: &Req<'_>, member: &str) -> Result<Value, ApiError> {
        let arn = r.s("RoleArn");
        let name = role_name_of(arn)?;
        let duration = r.int("DurationSeconds").unwrap_or(DEFAULT_ROLE_SECS);
        let known = self.entity(Kind::Role, name).ok();
        if duration < MIN_SECS || duration > known.map_or(MAX_ROLE_SECS, |e| e.max_session_secs) {
            return Err(validation("DurationSeconds is out of range for this role."));
        }
        let claims = claims_of(r.s(member));
        let session_name = r.opt("RoleSessionName").unwrap_or("saml-session");
        let role_id = known.map_or_else(|| new_id("AROA"), |e| Ok(e.id.clone()))?;
        let s = self.session(
            Who {
                kind: SessionKind::Role,
                subject: arn,
                name: session_name,
                id: format!("{role_id}:{session_name}"),
                secs: duration,
            },
            r,
        )?;
        let assumed = json!({"AssumedRoleId": s.id, "Arn": format!("arn:aws:sts::{}:assumed-role/{name}/{session_name}", r.account())});
        let text = |k: &str| {
            claims
                .get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        if member == "WebIdentityToken" {
            return Ok(json!({
                "Credentials": self.credentials(&s), "AssumedRoleUser": assumed,
                "SubjectFromWebIdentityToken": text("sub"), "Provider": text("iss"), "Audience": text("aud"),
            }));
        }
        Ok(json!({
            "Credentials": self.credentials(&s), "AssumedRoleUser": assumed,
            "Subject": text("sub"), "SubjectType": "persistent", "Issuer": text("iss"), "Audience": text("aud"),
        }))
    }

    fn get_session_token(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let duration = r.int("DurationSeconds").unwrap_or(DEFAULT_USER_SECS);
        if !(MIN_SECS..=MAX_USER_SECS).contains(&duration) {
            return Err(validation(
                "DurationSeconds must be between 900 and 129600.",
            ));
        }
        let who = self.identify(r);
        let s = self.session(
            Who {
                kind: SessionKind::User,
                subject: &who.arn,
                name: "",
                id: who.user_id,
                secs: duration,
            },
            r,
        )?;
        Ok(json!({"Credentials": self.credentials(&s)}))
    }

    fn get_federation_token(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let duration = r.int("DurationSeconds").unwrap_or(DEFAULT_USER_SECS);
        if !(MIN_SECS..=MAX_USER_SECS).contains(&duration) {
            return Err(validation(
                "DurationSeconds must be between 900 and 129600.",
            ));
        }
        let name = r.s("Name");
        let s = self.session(
            Who {
                kind: SessionKind::Federated,
                subject: name,
                name,
                id: format!("{}:{name}", r.account()),
                secs: duration,
            },
            r,
        )?;
        Ok(json!({
            "Credentials": self.credentials(&s),
            "FederatedUser": {"FederatedUserId": s.id, "Arn": format!("arn:aws:sts::{}:federated-user/{name}", r.account())},
        }))
    }

    fn get_access_key_info(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        // Every key in this account belongs to it; a key doze-aws did not make is told the same.
        Ok(json!({"Account": r.account()}))
    }
}

/// How a session shows as an identity.
fn session_identity(s: &Session) -> Identity {
    let arn = match s.kind {
        SessionKind::Role => {
            let role = s.subject.rsplit('/').next().unwrap_or_default();
            format!("arn:aws:sts::{}:assumed-role/{role}/{}", s.account, s.name)
        }
        SessionKind::User => s.subject.clone(),
        SessionKind::Federated => {
            format!("arn:aws:sts::{}:federated-user/{}", s.account, s.subject)
        }
    };
    Identity {
        arn,
        user_id: s.id.clone(),
        account: s.account.clone(),
    }
}

/// The role name in a role ARN (`arn:aws:iam::<account>:role/<path>/<name>`).
fn role_name_of(arn: &str) -> Result<&str, ApiError> {
    arn.split_once(":role/")
        .and_then(|(_, rest)| rest.rsplit('/').next())
        .filter(|n| !n.is_empty())
        .ok_or_else(|| validation(format!("Invalid role ARN: {arn}")))
}

/// The claims of a JWT, or an empty object for anything else.
fn claims_of(token: &str) -> Value {
    token
        .split('.')
        .nth(1)
        .and_then(|p| aws_core::base64::decode(&p.replace('-', "+").replace('_', "/")))
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null)
}
