//! Rotation: `RotateSecret` and `CancelRotateSecret`, and the run that drives a rotation function through its four steps.
//!
//! As on AWS, `RotateSecret` answers at once and the rotation runs after it: the service makes a version for the rotation's token
//! that has no value yet and carries `AWSPENDING` (AWS's rotation templates check for exactly that before `createSecret`), then
//! invokes the function over the peer bus for `createSecret`, `setSecret`, `testSecret` and `finishSecret` in turn. The function
//! calls back into this service like any client, so the run never holds the actor. A rotation is complete when `AWSCURRENT` is
//! moved to the token's version (what `finishSecret` does): that is when `LastRotatedDate` is set and `AWSPENDING` removed. A
//! step that fails stops the run and leaves the pending version, so the next `RotateSecret` is refused until it is cancelled, as on
//! AWS. Better than the Go edition, which ran the steps inside the request: here a slow or failing function cannot hold the caller,
//! and a function that reads the secret back is not deadlocked against the request that started it.
//!
//! Not built: the schedule (`AutomaticallyAfterDays`, `ScheduleExpression`) is stored and shown as `NextRotationDate`, and no timer
//! starts a rotation on it; `RotateImmediately: false` stores the configuration and does not run `testSecret`. Both are (memory)
//! descriptions of AWS behaviour and are recorded as open items.

use std::collections::BTreeSet;
use std::sync::Arc;

use aws_model_secrets_manager::keys::{
    CancelRotateSecretRequest as Cancel, CancelRotateSecretResponse as Cancelled,
    RotateSecretRequest as Rotate, RotateSecretResponse as Rotated, RotationRulesType as Rules,
};
use aws_peers::Peers;
use aws_wire::handler::{reply, request};
use aws_wire::service::{Call, Caller, Reply};
use serde_json::json;

use crate::core::{Core, invalid_request};
use crate::model::{CURRENT, DAY_MS, Rotation, Version};

/// The label of the version a rotation is making.
pub(crate) const PENDING: &str = "AWSPENDING";

/// The steps of a rotation, in order.
const STEPS: [&str; 4] = ["createSecret", "setSecret", "testSecret", "finishSecret"];

/// A rotation to run after the request that started it has been answered and saved.
#[derive(Clone, Debug)]
pub(crate) struct Run {
    pub(crate) function: String,
    pub(crate) secret_arn: String,
    pub(crate) token: String,
    pub(crate) caller: Caller,
}

impl Core {
    pub(crate) fn rotate_secret(&mut self, call: &Call) -> Reply {
        let v = request::<Rotate>(call)?;
        let name = self.locate_live(v.get(Rotate::SECRET_ID).unwrap_or_default())?;
        let mut r = self.rotations.get(&name).cloned().unwrap_or_default();
        if let Some(arn) = v.get(Rotate::ROTATION_LAMBDA_ARN) {
            r.function = Some(arn.to_owned());
        }
        if let Some(rules) = v.get(Rotate::ROTATION_RULES) {
            r.after_days = rules.get(Rules::AUTOMATICALLY_AFTER_DAYS);
            r.duration = rules.get(Rules::DURATION).map(str::to_owned);
            r.schedule = rules.get(Rules::SCHEDULE_EXPRESSION).map(str::to_owned);
        }
        let Some(function) = r.function.clone() else {
            // (memory)
            return Err(invalid_request(
                "No Lambda rotation function ARN is associated with this secret.",
            ));
        };
        let now = self.now_ms();
        let secret = self
            .secrets
            .get_mut(&name)
            .ok_or_else(crate::core::not_found)?;
        if let Some(p) = &r.pending
            && secret
                .versions
                .get(p)
                .is_some_and(|x| x.stages.contains(PENDING))
        {
            // (memory)
            return Err(invalid_request(
                "A previous rotation isn't complete. That rotation will be reattempted.",
            ));
        }
        let token = match v.get(Rotate::CLIENT_REQUEST_TOKEN) {
            Some(t) => t.to_owned(),
            None => {
                aws_seal::uuid_v4().ok_or_else(|| aws_wire::ApiError::internal("no randomness"))?
            }
        };
        let immediately = v.get(Rotate::ROTATE_IMMEDIATELY).unwrap_or(true);
        r.enabled = true;
        r.configured_ms = Some(now);
        if immediately {
            open_pending(secret, &token, now);
            r.pending = Some(token.clone());
            self.run = Some(Run {
                function,
                secret_arn: secret.arn.clone(),
                token: token.clone(),
                caller: call.caller.clone(),
            });
        }
        let arn = secret.arn.clone();
        self.rotations.insert(name.clone(), r);
        reply::<Rotated>(|o| {
            o.put(Rotated::ARN, &arn);
            o.put(Rotated::NAME, &name);
            o.put(Rotated::VERSION_ID, &token);
        })
    }

    pub(crate) fn cancel_rotate_secret(&mut self, call: &Call) -> Reply {
        let v = request::<Cancel>(call)?;
        let name = self.locate_live(v.get(Cancel::SECRET_ID).unwrap_or_default())?;
        let mut r = self.rotations.get(&name).cloned().unwrap_or_default();
        let secret = self
            .secrets
            .get_mut(&name)
            .ok_or_else(crate::core::not_found)?;
        let pending = r.pending.take();
        if let Some(id) = &pending {
            let empty = secret.versions.get(id).is_some_and(|x| x.sealed.is_empty());
            if empty {
                secret.versions.remove(id);
            } else if let Some(x) = secret.versions.get_mut(id) {
                x.stages.remove(PENDING);
            }
        }
        r.enabled = false;
        let arn = secret.arn.clone();
        self.rotations.insert(name.clone(), r);
        reply::<Cancelled>(|o| {
            o.put(Cancelled::ARN, &arn);
            o.put(Cancelled::NAME, &name);
            if let Some(id) = &pending {
                o.put(Cancelled::VERSION_ID, id);
            }
        })
    }

    /// After a staging label moved: a rotation is complete when `AWSCURRENT` reaches its token's version.
    pub(crate) fn note_stage_moved(&mut self, name: &str, stage: &str, to: Option<&str>) {
        if stage != CURRENT {
            return;
        }
        let Some(r) = self.rotations.get(name) else {
            return;
        };
        if r.pending.as_deref().is_none() || r.pending.as_deref() != to {
            return;
        }
        let now = self.now_ms();
        let mut r = r.clone();
        r.pending = None;
        r.last_ms = Some(now);
        if let (Some(id), Some(secret)) = (to, self.secrets.get_mut(name))
            && let Some(version) = secret.versions.get_mut(id)
        {
            version.stages.remove(PENDING);
        }
        self.rotations.insert(name.to_owned(), r);
    }
}

impl Rotation {
    /// When the next rotation is due, for `NextRotationDate`: the last rotation (or the configuration) plus the days of the rule.
    /// A `ScheduleExpression` of the form `rate(N days)` or `rate(N hours)` is read too; a `cron()` one is not computed. (memory)
    pub(crate) fn next_ms(&self) -> Option<i64> {
        if !self.enabled {
            return None;
        }
        let base = self.last_ms.or(self.configured_ms)?;
        if let Some(days) = self.after_days {
            return Some(base + days * DAY_MS);
        }
        let inner = self
            .schedule
            .as_deref()?
            .strip_prefix("rate(")?
            .strip_suffix(')')?;
        let (n, unit) = inner.trim().split_once(' ')?;
        let n: i64 = n.parse().ok()?;
        let unit_ms = match unit.trim_end_matches('s') {
            "day" => DAY_MS,
            "hour" => 3_600_000,
            _ => return None,
        };
        Some(base + n * unit_ms)
    }
}

/// Drives the rotation function through its steps. A step that fails (a call that fails, or a function error) stops the run.
pub(crate) async fn run(peers: Arc<Peers>, run: Run) {
    for step in STEPS {
        let payload =
            json!({"Step": step, "SecretId": run.secret_arn, "ClientRequestToken": run.token});
        let body = json!({
            "FunctionName": run.function,
            "InvocationType": "RequestResponse",
            "Payload": aws_core::base64::encode(payload.to_string().as_bytes()),
        });
        match peers
            .call_rest("secretsmanager", &run.caller, "lambda", "Invoke", &body)
            .await
        {
            Ok(reply) if reply.json.get("FunctionError").is_none() => {}
            Ok(reply) => {
                tracing::warn!(
                    secret = %run.secret_arn,
                    step,
                    error = %reply.json.get("FunctionError").and_then(|e| e.as_str()).unwrap_or("Unhandled"),
                    "secretsmanager: the rotation function failed; the rotation stops with AWSPENDING in place"
                );
                return;
            }
            Err(e) => {
                tracing::warn!(
                    secret = %run.secret_arn,
                    step,
                    error = %e.message,
                    "secretsmanager: the rotation function could not be invoked; the rotation stops with AWSPENDING in place"
                );
                return;
            }
        }
    }
}

/// Puts `AWSPENDING` on the version the rotation function will fill under `token`, made with no value (an empty sealed value is
/// never a real one).
fn open_pending(secret: &mut crate::model::Secret, token: &str, now: i64) {
    for version in secret.versions.values_mut() {
        version.stages.remove(PENDING);
    }
    let stub = secret
        .versions
        .entry(token.to_owned())
        .or_insert_with(|| Version {
            sealed: Vec::new(),
            binary: false,
            stages: BTreeSet::new(),
            created_ms: now,
            accessed_day_ms: None,
        });
    stub.stages.insert(PENDING.to_owned());
    secret.changed_ms = now;
}

/// Whether a version holds a value yet (a rotation's pending version does not until `createSecret` puts one).
pub(crate) fn has_value(v: &Version) -> bool {
    !v.sealed.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_rotation_follows_the_rule() {
        let mut r = Rotation {
            enabled: true,
            configured_ms: Some(0),
            after_days: Some(30),
            ..Rotation::default()
        };
        assert_eq!(r.next_ms(), Some(30 * DAY_MS));
        r.last_ms = Some(DAY_MS);
        assert_eq!(r.next_ms(), Some(31 * DAY_MS));
        r.after_days = None;
        r.schedule = Some("rate(4 hours)".into());
        assert_eq!(r.next_ms(), Some(DAY_MS + 4 * 3_600_000));
        r.schedule = Some("cron(0 16 1,15 * ? *)".into());
        assert_eq!(r.next_ms(), None);
        r.enabled = false;
        r.schedule = Some("rate(1 day)".into());
        assert_eq!(r.next_ms(), None);
    }
}
