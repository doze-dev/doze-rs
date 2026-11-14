//! Running as the state machine's role. A task call is made with credentials for the execution role: an STS session that IAM
//! resolves, so under `--iam enforce` a task may do what the role allows and nothing else, and under `--iam soft` a call the role does
//! not allow is logged. The session is made with `AssumeRole` over the peer bus (Step Functions acts as itself, so the trust policy
//! is not asked) and kept until shortly before it ends.
//!
//! Whether AWS would accept the role is checked when a machine is created or re-pointed: it must exist and trust
//! `states.amazonaws.com`. A role IAM has never heard of is refused only when IAM enforces (as Lambda does: a local account is often
//! made without the roles a template names); the tasks then run as the caller who started the execution.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use aws_peers::{Mode, Peers};
use aws_wire::ApiError;
use aws_wire::service::Caller;
use serde_json::{Value, json};

fn role_name(arn: &str) -> Option<&str> {
    arn.split_once(":role/")
        .map(|(_, rest)| rest.rsplit('/').next().unwrap_or(rest))
}

/// Whether a trust policy lets Step Functions assume the role.
pub(crate) fn trusts_states(document: &str) -> bool {
    let decoded = aws_core::uri::decode_str(document)
        .map_or_else(|| document.to_owned(), std::borrow::Cow::into_owned);
    let Ok(doc) = serde_json::from_str::<Value>(&decoded) else {
        return false;
    };
    let statements = match doc.get("Statement") {
        Some(Value::Array(a)) => a.clone(),
        Some(one @ Value::Object(_)) => vec![one.clone()],
        _ => return false,
    };
    let names = |v: Option<&Value>, want: &str| match v {
        Some(Value::String(s)) => {
            s == want || s.starts_with("states.") && want == "states.amazonaws.com"
        }
        Some(Value::Array(a)) => a.iter().any(|x| {
            x.as_str()
                .is_some_and(|s| s == want || s.starts_with("states."))
        }),
        _ => false,
    };
    statements.iter().any(|s| {
        s.get("Effect").and_then(Value::as_str) == Some("Allow")
            && match s.get("Principal") {
                Some(Value::String(p)) => p == "*",
                Some(p) => {
                    names(p.get("Service"), "states.amazonaws.com") || names(p.get("AWS"), "*")
                }
                None => false,
            }
    })
}

/// Refuses a role AWS would refuse, when IAM can say and enforces.
pub(crate) async fn check(
    peers: Option<&Peers>,
    caller: &Caller,
    role: &str,
) -> Result<(), ApiError> {
    let Some(peers) = peers.filter(|p| p.mode() != Mode::Off) else {
        return Ok(());
    };
    let Some(name) = role_name(role) else {
        return Ok(());
    };
    let got = peers
        .call(
            "states",
            caller,
            "iam",
            "GetRole",
            &json!({"RoleName": name}),
        )
        .await;
    let fine = match got {
        Ok(v) => v
            .pointer("/Role/AssumeRolePolicyDocument")
            .and_then(Value::as_str)
            .is_none_or(trusts_states),
        Err(e) if e.code.starts_with("NoSuchEntity") => false,
        Err(_) => true,
    };
    if fine || peers.mode() != Mode::Enforce {
        if !fine {
            tracing::warn!("iam soft mode: Step Functions could not assume {role} on AWS");
        }
        return Ok(());
    }
    // (memory)
    Err(ApiError::modeled(
        "AccessDeniedException",
        format!(
            "Neither the global service principal states.amazonaws.com, nor the regional one is authorized to assume the provided role: '{role}'"
        ),
    ))
}

/// Credentials for a role.
#[derive(Clone, Debug)]
struct Session {
    key: String,
    token: String,
    expires_ms: i64,
}

/// The sessions made so far, by role.
#[derive(Debug, Default)]
pub(crate) struct Sessions {
    made: Mutex<HashMap<String, Session>>,
}

impl Sessions {
    /// The caller a task call is made as: the role's session, or `fallback` (the starter of the execution) when there is no role
    /// to assume or nothing to assume it.
    pub(crate) async fn caller(
        &self,
        peers: &Peers,
        fallback: &Caller,
        role: &str,
        now_ms: i64,
    ) -> Caller {
        if peers.mode() == Mode::Off || role.is_empty() {
            return fallback.clone();
        }
        let cached = self
            .made
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(role)
            .cloned();
        let session = match cached.filter(|s| s.expires_ms - now_ms > 600_000) {
            Some(s) => s,
            None => match Self::assume(peers, fallback, role, now_ms).await {
                Some(s) => {
                    self.made
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(role.to_owned(), s.clone());
                    s
                }
                None => return fallback.clone(),
            },
        };
        Caller {
            access_key_id: session.key,
            session_token: Some(session.token),
            service: None,
            ..fallback.clone()
        }
    }

    async fn assume(peers: &Peers, caller: &Caller, role: &str, now_ms: i64) -> Option<Session> {
        let ask = json!({"RoleArn": role, "RoleSessionName": format!("states-{now_ms}"), "DurationSeconds": 3600});
        let got = match peers
            .call("states", caller, "sts", "AssumeRole", &ask)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!("could not assume {role} for a task: {}", e.message);
                return None;
            }
        };
        let c = got.get("Credentials")?;
        let text = |k: &str| {
            c.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let expires_ms = match c.get("Expiration") {
            Some(Value::Number(n)) => n.as_f64().map_or(0, |s| (s * 1000.0) as i64),
            Some(Value::String(s)) => aws_core::time::parse_rfc3339(s).map_or(0, |s| s * 1000),
            _ => 0,
        };
        Some(Session {
            key: text("AccessKeyId"),
            token: text("SessionToken"),
            expires_ms,
        })
    }
}
