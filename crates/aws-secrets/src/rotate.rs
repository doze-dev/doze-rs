//! Rotation: `RotateSecret` and `CancelRotateSecret`, and the run that drives a rotation function through its steps.
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
//! `RotateImmediately: false` does what the model documents: the configuration is stored, the next rotation waits for the schedule's
//! window (`schedule.rs` starts it), and the configuration is tested by running the function's `testSecret` step alone against an
//! `AWSPENDING` version that is then removed. The pending version holds a copy of the current value (what AWS puts in it is not
//! documented: (memory)), so a `testSecret` that logs in with it succeeds.

use std::collections::BTreeSet;
use std::sync::Arc;

use aws_model_secrets_manager::keys::{
    CancelRotateSecretRequest as Cancel, CancelRotateSecretResponse as Cancelled,
    RotateSecretRequest as Rotate, RotateSecretResponse as Rotated, RotationRulesType as Rules,
};
use aws_peers::Peers;
use aws_wire::ApiError;
use aws_wire::handler::{reply, request};
use aws_wire::service::{Call, Caller, Reply};
use serde_json::json;

use crate::core::{Core, invalid_request};
use crate::model::{CURRENT, Rotation, Version};

/// The label of the version a rotation is making.
pub(crate) const PENDING: &str = "AWSPENDING";

/// The steps of a rotation, in order.
const STEPS: [&str; 4] = ["createSecret", "setSecret", "testSecret", "finishSecret"];

/// What a run invokes: the whole rotation, or `testSecret` alone (a `RotateImmediately: false` check of the configuration).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    All,
    TestOnly,
}

/// A rotation to run after the request that started it has been answered and saved.
#[derive(Clone, Debug)]
pub(crate) struct Run {
    pub(crate) function: String,
    pub(crate) secret_arn: String,
    pub(crate) token: String,
    pub(crate) caller: Caller,
    pub(crate) step: Step,
}

impl Core {
    pub(crate) async fn rotate_secret(&mut self, call: &Call) -> Reply {
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
            crate::schedule::check_rules(&r)?;
        }
        let Some(function) = r.function.clone() else {
            // (memory)
            return Err(invalid_request(
                "No Lambda rotation function ARN is associated with this secret.",
            ));
        };
        let secret = self.secrets.get(&name).ok_or_else(crate::core::not_found)?;
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
        let arn = secret.arn.clone();
        let token = match v.get(Rotate::CLIENT_REQUEST_TOKEN) {
            Some(t) => t.to_owned(),
            None => aws_seal::uuid_v4().ok_or_else(|| ApiError::internal("no randomness"))?,
        };
        let immediately = v.get(Rotate::ROTATE_IMMEDIATELY).unwrap_or(true);
        r.enabled = true;
        r.pending = None;
        r.configured_ms = Some(self.now_ms());
        self.rotations.insert(name.clone(), r);
        if immediately {
            self.begin(&name, token.clone(), call.caller.clone());
        } else {
            self.open_test(call, &name, &function, &token).await?;
        }
        reply::<Rotated>(|o| {
            o.put(Rotated::ARN, &arn);
            o.put(Rotated::NAME, &name);
            o.put(Rotated::VERSION_ID, &token);
        })
    }

    /// For `RotateImmediately: false`: an `AWSPENDING` version under `token` holding the current value, and a run of `testSecret`
    /// alone that removes the label afterwards. A secret with no current value has nothing to test with, and nothing is run.
    async fn open_test(
        &mut self,
        call: &Call,
        name: &str,
        function: &str,
        token: &str,
    ) -> Result<(), ApiError> {
        let secret = self.secrets.get(name).ok_or_else(crate::core::not_found)?;
        if secret.versions.contains_key(token) {
            return Ok(());
        }
        let Some((current, version)) = secret.staged(CURRENT) else {
            return Ok(());
        };
        let plain = self
            .kms_open(call, secret, current, &version.sealed)
            .await?;
        let binary = version.binary;
        let sealed = self.kms_seal(call, secret, token, &plain).await?;
        let now = self.now_ms();
        let secret = self
            .secrets
            .get_mut(name)
            .ok_or_else(crate::core::not_found)?;
        for version in secret.versions.values_mut() {
            version.stages.remove(PENDING);
        }
        secret.versions.insert(
            token.to_owned(),
            Version {
                sealed,
                binary,
                stages: BTreeSet::from([PENDING.to_owned()]),
                created_ms: now,
                accessed_day_ms: None,
            },
        );
        self.runs.push(Run {
            function: function.to_owned(),
            secret_arn: secret.arn.clone(),
            token: token.to_owned(),
            caller: call.caller.clone(),
            step: Step::TestOnly,
        });
        Ok(())
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
        let mut r: Rotation = r.clone();
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

/// Drives the rotation function through its steps. A step that fails (a call that fails, or a function error) stops the run. A
/// `testSecret`-only run then takes `AWSPENDING` off the version it tested, whatever the step said.
pub(crate) async fn run(peers: Arc<Peers>, run: Run) {
    let steps: &[&str] = match run.step {
        Step::All => &STEPS,
        Step::TestOnly => &["testSecret"],
    };
    for step in steps {
        if !invoke(&peers, &run, step).await {
            break;
        }
    }
    if run.step == Step::TestOnly {
        let body = json!({"SecretId": run.secret_arn, "VersionStage": PENDING, "RemoveFromVersionId": run.token});
        if let Err(e) = peers
            .call(
                "secretsmanager",
                &run.caller,
                "secretsmanager",
                "UpdateSecretVersionStage",
                &body,
            )
            .await
        {
            tracing::warn!(secret = %run.secret_arn, error = %e.message, "secretsmanager: AWSPENDING could not be removed after testSecret");
        }
    }
}

/// Invokes one step; `false` when it failed (and says so in the log).
async fn invoke(peers: &Peers, run: &Run, step: &str) -> bool {
    let payload =
        json!({"Step": step, "SecretId": run.secret_arn, "ClientRequestToken": run.token});
    let body = json!({
        "FunctionName": run.function,
        "InvocationType": "RequestResponse",
        "Payload": aws_core::base64::encode(payload.to_string().as_bytes()),
    });
    let error = match peers
        .call_rest("secretsmanager", &run.caller, "lambda", "Invoke", &body)
        .await
    {
        Ok(reply) => match reply.json.get("FunctionError") {
            None => return true,
            Some(e) => e.as_str().unwrap_or("Unhandled").to_owned(),
        },
        Err(e) => e.message,
    };
    tracing::warn!(
        secret = %run.secret_arn,
        step,
        error = %error,
        "secretsmanager: the rotation function failed; the rotation stops with AWSPENDING in place"
    );
    false
}

/// Puts `AWSPENDING` on the version the rotation function will fill under `token`, made with no value (an empty sealed value is
/// never a real one).
pub(crate) fn open_pending(secret: &mut crate::model::Secret, token: &str, now: i64) {
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
