//! Invoking as the schedule's execution role. Scheduler assumes the role (`RoleArn` of the target) and calls the target with the
//! role's credentials, so the target service asks IAM about the role: under `--iam enforce` an invocation may do what the role allows
//! and nothing else; under `--iam soft` what it would be refused is logged and goes through; under `--iam off` nothing is asked.
//!
//! AWS requires the role to trust `scheduler.amazonaws.com`. A role IAM does not know, or one that does not trust Scheduler, fails
//! the invocation under enforce (and is dead-lettered like any failure); under soft the log says so and the invocation is made as
//! Scheduler itself. The session is made with `AssumeRole` over the bus and kept until shortly before it ends.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, PoisonError};

use aws_peers::{Mode, Peers};
use aws_wire::service::Caller;
use serde_json::{Value, json};

use crate::deliver::{Failure, Outcome};

/// Credentials for a role.
#[derive(Clone, Debug)]
struct Session {
    key: String,
    token: String,
    expires_ms: i64,
}

/// The sessions made so far, by role, and the roles already warned about under soft (said once, not on every firing).
#[derive(Debug, Default)]
pub(crate) struct Sessions {
    made: Mutex<HashMap<String, Session>>,
    warned: Mutex<HashSet<String>>,
}

impl Sessions {
    /// The caller an invocation is made as: the role's session; Scheduler itself when IAM is off, or (logged) when the role cannot
    /// be assumed under soft. `Err` is the failure of an invocation whose role cannot be assumed under enforce.
    pub(crate) async fn caller(
        &self,
        peers: &Peers,
        base: &Caller,
        role: &str,
        now_ms: i64,
    ) -> Result<Caller, Outcome> {
        let as_service = Caller {
            service: Some("scheduler".to_owned()),
            ..base.clone()
        };
        if peers.mode() == Mode::Off {
            return Ok(as_service);
        }
        let cached = self.lock().get(role).cloned();
        let session = match cached.filter(|s| s.expires_ms - now_ms > 600_000) {
            Some(s) => s,
            None => match assume(peers, base, role, now_ms).await {
                Ok(s) => {
                    self.lock().insert(role.to_owned(), s.clone());
                    s
                }
                Err(why) if peers.mode() == Mode::Enforce => {
                    return Err(Failure::fatal("AccessDeniedException", why));
                }
                Err(why) => {
                    let first = self
                        .warned
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(role.to_owned());
                    if first {
                        tracing::warn!(
                            "iam soft mode: EventBridge Scheduler invocations as {role} would fail under --iam enforce: {why} (said once per role; invocations go on as Scheduler itself)"
                        );
                    }
                    return Ok(as_service);
                }
            },
        };
        Ok(Caller {
            access_key_id: session.key,
            session_token: Some(session.token),
            service: None,
            ..base.clone()
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Session>> {
        self.made.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Checks the role trusts Scheduler and makes a session for it. The error says what to fix.
async fn assume(
    peers: &Peers,
    caller: &Caller,
    role: &str,
    now_ms: i64,
) -> Result<Session, String> {
    let name = role
        .split_once(":role/")
        .map(|(_, rest)| rest.rsplit('/').next().unwrap_or(rest))
        .ok_or_else(|| format!("{role} is not a role ARN"))?;
    let got = peers
        .call("scheduler", caller, "iam", "GetRole", &json!({"RoleName": name}))
        .await
        .map_err(|e| format!("the execution role {role} cannot be read ({}): create it, trusting scheduler.amazonaws.com", e.message))?;
    let trusted = got
        .pointer("/Role/AssumeRolePolicyDocument")
        .and_then(Value::as_str)
        .is_none_or(trusts_scheduler);
    if !trusted {
        // (memory) the wording of AWS's refusal is not recorded.
        return Err(format!(
            "the execution role {role} does not trust scheduler.amazonaws.com: add it as a Service principal of the role's trust policy"
        ));
    }
    let ask = json!({"RoleArn": role, "RoleSessionName": format!("scheduler-{now_ms}"), "DurationSeconds": 3600});
    let got = peers
        .call("scheduler", caller, "sts", "AssumeRole", &ask)
        .await
        .map_err(|e| {
            format!(
                "the execution role {role} could not be assumed: {}",
                e.message
            )
        })?;
    let c = got.get("Credentials").cloned().unwrap_or(Value::Null);
    let text = |k: &str| {
        c.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    #[allow(clippy::cast_possible_truncation)]
    // WHY: seconds since the epoch in ms fit an i64 by far.
    let expires_ms = match c.get("Expiration") {
        Some(Value::Number(n)) => n.as_f64().map_or(0, |s| (s * 1000.0) as i64),
        Some(Value::String(s)) => aws_core::time::parse_rfc3339(s).map_or(0, |s| s * 1000),
        _ => 0,
    };
    Ok(Session {
        key: text("AccessKeyId"),
        token: text("SessionToken"),
        expires_ms,
    })
}

/// Whether a trust policy lets EventBridge Scheduler assume the role.
pub(crate) fn trusts_scheduler(document: &str) -> bool {
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
        Some(Value::String(s)) => s == want,
        Some(Value::Array(a)) => a.iter().any(|x| x.as_str() == Some(want)),
        _ => false,
    };
    statements.iter().any(|s| {
        s.get("Effect").and_then(Value::as_str) == Some("Allow")
            && match s.get("Principal") {
                Some(Value::String(p)) => p == "*",
                Some(p) => {
                    names(p.get("Service"), "scheduler.amazonaws.com") || names(p.get("AWS"), "*")
                }
                None => false,
            }
    })
}
