//! What a checkpoint's updates do to an execution, and the history events each change writes. The transitions are those of AWS's
//! checkpoint server (`@aws/durable-execution-sdk-js-testing` 1.3.2, `storage/checkpoint-manager`): a new operation starts
//! `STARTED` (a step retried at once starts `PENDING`), `SUCCEED` and `FAIL` close it, `RETRY` makes a step `PENDING` until its next
//! attempt is due, `CANCEL` ends a wait. An update of type `EXECUTION` is the execution's own, whatever id it carries.

use serde_json::{Value, json};

use super::check::s;
use super::shown::{event_error, payload, secs};
use super::{Chained, Execution, Op, callback_id};

impl Execution {
    /// Writes a history event; `details` go under `<EventType>Details`.
    pub(crate) fn event(&mut self, now: i64, kind: &str, op: Option<&Op>, details: Value) {
        let mut e = json!({
            "EventType": kind,
            "EventId": self.events.len() + 1,
            "EventTimestamp": secs(now),
        });
        if let Some(op) = op {
            crate::set(&mut e, "Id", json!(op.id));
            // Recorded 2026-11-19: an execution's own events carry the execution's name.
            if op.kind == "EXECUTION" && op.name.is_none() {
                crate::set(&mut e, "Name", json!(self.name));
            }
            for (k, v) in [
                ("SubType", &op.sub),
                ("Name", &op.name),
                ("ParentId", &op.parent),
            ] {
                if let Some(v) = v {
                    crate::set(&mut e, k, json!(v));
                }
            }
        }
        crate::set(&mut e, &format!("{kind}Details"), details);
        self.events.push(e.to_string());
    }

    pub(crate) fn mark(&mut self, id: &str) {
        if !self.dirty.iter().any(|d| d == id) {
            self.dirty.push(id.to_owned());
        }
    }

    pub(crate) fn op_mut(&mut self, id: &str) -> Option<&mut Op> {
        self.ops.iter_mut().find(|o| o.id == id)
    }

    /// Ends the execution with `status`, its result or error, and the event that says so.
    pub(crate) fn close(
        &mut self,
        now: i64,
        status: &str,
        result: Option<String>,
        error: Option<String>,
    ) {
        self.status = status.to_owned();
        self.end_ms = Some(now);
        self.wake = false;
        self.retry_at = None;
        let kind = match status {
            "SUCCEEDED" => "ExecutionSucceeded",
            "FAILED" => "ExecutionFailed",
            "TIMED_OUT" => "ExecutionTimedOut",
            _ => "ExecutionStopped",
        };
        let details = if status == "SUCCEEDED" {
            json!({"Result": payload(result.as_ref())})
        } else {
            json!({"Error": event_error(error.as_deref())})
        };
        let exec = self.ops.first().cloned();
        if let Some(op) = self.ops.first_mut() {
            op.status = status.to_owned();
            op.end_ms = Some(now);
        }
        self.event(now, kind, exec.as_ref(), details);
        self.result = result;
        self.error = error;
    }

    /// Applies one checked update. A chained invoke it starts is returned, to be made outside the actor.
    pub(crate) fn apply(&mut self, u: &Value, now: i64) -> Option<Chained> {
        let id = s(u, "Id").unwrap_or_default().to_owned();
        if s(u, "Type") == Some("EXECUTION") {
            let result = s(u, "Payload").map(str::to_owned);
            let failed = s(u, "Action") == Some("FAIL");
            let err = u
                .get("Error")
                .filter(|v| !v.is_null())
                .map(Value::to_string);
            let (status, result, err) = if failed {
                ("FAILED", None, err)
            } else {
                ("SUCCEEDED", result, None)
            };
            self.close(now, status, result, err);
            return None;
        }
        let existing = self.ops.iter().any(|o| o.id == id);
        let chained = if existing {
            self.update(&id, u, now)
        } else {
            self.start(&id, u, now)
        };
        self.mark(&id);
        chained
    }

    fn start(&mut self, id: &str, u: &Value, now: i64) -> Option<Chained> {
        let text = |k: &str| s(u, k).map(str::to_owned);
        let mut op = Op {
            id: id.to_owned(),
            parent: text("ParentId"),
            name: text("Name"),
            kind: text("Type").unwrap_or_default(),
            sub: text("SubType"),
            status: "STARTED".to_owned(),
            start_ms: now,
            ..Op::default()
        };
        let action = s(u, "Action").unwrap_or_default();
        let mut chained = None;
        let started = match op.kind.as_str() {
            "WAIT" => {
                let secs = u
                    .get("WaitOptions")
                    .and_then(|o| o.get("WaitSeconds"))
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let end = now + self.scaled(secs);
                (op.wait_secs, op.wait_end_ms) = (Some(secs), Some(end));
                (
                    "WaitStarted",
                    json!({"Duration": secs, "ScheduledEndTimestamp": super::shown::secs(end)}),
                )
            }
            "CALLBACK" => start_callback(&mut op, &self.arn, u, now),
            "CHAINED_INVOKE" => {
                let (c, started) = self.start_chained(&mut op, u);
                chained = Some(c);
                started
            }
            "CONTEXT" => ("ContextStarted", json!({})),
            _ => ("StepStarted", json!({})),
        };
        self.ops.push(op);
        if matches!(action, "SUCCEED" | "FAIL" | "RETRY") {
            self.update(id, u, now);
        } else {
            let op = self.ops.last().cloned();
            self.event(now, started.0, op.as_ref(), started.1);
        }
        chained
    }

    /// A chained invoke's operation, and the invoke to make.
    fn start_chained(&self, op: &mut Op, u: &Value) -> (Chained, (&'static str, Value)) {
        let target = u
            .get("ChainedInvokeOptions")
            .and_then(|o| o.get("FunctionName"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        op.target = Some(target.clone());
        op.input = s(u, "Payload").map(str::to_owned);
        let chained = Chained {
            execution: self.arn.clone(),
            op: op.id.clone(),
            function: target.clone(),
            payload: op.input.clone().unwrap_or_else(|| "{}".to_owned()),
            account: self.account.clone(),
            endpoint: self.endpoint.clone(),
        };
        let started = json!({"FunctionName": target, "Input": payload(op.input.as_ref())});
        (chained, ("ChainedInvokeStarted", started))
    }

    /// Milliseconds of a wait or delay of `secs` seconds, scaled.
    fn scaled(&self, secs: i64) -> i64 {
        let scale = if self.scale.is_finite() && self.scale >= 0.0 {
            self.scale
        } else {
            1.0
        };
        ((secs as f64) * 1000.0 * scale).round() as i64
    }

    fn update(&mut self, id: &str, u: &Value, now: i64) -> Option<Chained> {
        let action = s(u, "Action").unwrap_or_default().to_owned();
        let result = s(u, "Payload").map(str::to_owned);
        let err = u
            .get("Error")
            .filter(|v| !v.is_null())
            .map(Value::to_string);
        let delay = u
            .get("StepOptions")
            .and_then(|o| o.get("NextAttemptDelaySeconds"))
            .and_then(Value::as_i64);
        let replay = u
            .get("ContextOptions")
            .and_then(|o| o.get("ReplayChildren"))
            .and_then(Value::as_bool)
            == Some(true);
        let wait_after = delay.map(|d| self.scaled(d));
        let op = self.op_mut(id)?;
        let event = match (op.kind.as_str(), action.as_str()) {
            ("WAIT", "CANCEL") => {
                op.status = "CANCELLED".into();
                op.end_ms = Some(now);
                Some((
                    "WaitCancelled",
                    json!({"Error": event_error(err.as_deref())}),
                ))
            }
            ("STEP", "START") => {
                op.status = "STARTED".into();
                op.next_attempt_ms = None;
                Some(("StepStarted", json!({})))
            }
            ("STEP" | "CONTEXT", "SUCCEED" | "FAIL") => Some(finish(
                op,
                action == "SUCCEED",
                (result, err),
                replay,
                now,
                delay,
            )),
            ("STEP", "RETRY") => {
                op.attempt += 1;
                op.status = "PENDING".into();
                op.next_attempt_ms = Some(now + wait_after.unwrap_or(0));
                let retry = json!({"CurrentAttempt": op.attempt, "NextAttemptDelaySeconds": delay});
                let failed = err.is_some();
                (op.result, op.error) = (result.clone(), err.clone());
                Some(if failed {
                    (
                        "StepFailed",
                        json!({"Error": event_error(err.as_deref()), "RetryDetails": retry}),
                    )
                } else {
                    (
                        "StepSucceeded",
                        json!({"Result": payload(result.as_ref()), "RetryDetails": retry}),
                    )
                })
            }
            _ => None,
        };
        if let Some((kind, details)) = event {
            let op = self.ops.iter().find(|o| o.id == id).cloned();
            self.event(now, kind, op.as_ref(), details);
        }
        None
    }
}

/// A callback's operation: its id, and the deadlines its timeouts set (0 is none: the model's `CallbackOptions`).
fn start_callback(op: &mut Op, execution: &str, u: &Value, now: i64) -> (&'static str, Value) {
    let num = |key: &str| {
        u.get("CallbackOptions")
            .and_then(|o| o.get(key))
            .and_then(Value::as_i64)
            .unwrap_or(0)
    };
    let (timeout, beat) = (num("TimeoutSeconds"), num("HeartbeatTimeoutSeconds"));
    let cb = callback_id(execution, &op.id);
    op.callback_id = Some(cb.clone());
    (op.callback_timeout, op.callback_heartbeat) = (timeout, beat);
    op.callback_deadline_ms = (timeout > 0).then(|| now + timeout * 1000);
    op.heartbeat_deadline_ms = (beat > 0).then(|| now + beat * 1000);
    let mut d = json!({"CallbackId": cb});
    if timeout > 0 {
        crate::set(&mut d, "Timeout", json!(timeout));
    }
    if beat > 0 {
        crate::set(&mut d, "HeartbeatTimeout", json!(beat));
    }
    ("CallbackStarted", d)
}

/// Closes a step or a context with its result or error.
fn finish(
    op: &mut Op,
    ok: bool,
    (result, err): (Option<String>, Option<String>),
    replay: bool,
    now: i64,
    delay: Option<i64>,
) -> (&'static str, Value) {
    op.status = if ok { "SUCCEEDED" } else { "FAILED" }.into();
    op.end_ms = Some(now);
    op.next_attempt_ms = None;
    let step = op.kind == "STEP";
    if step {
        op.attempt += 1;
    } else {
        op.replay_children = replay;
    }
    let retry = json!({"CurrentAttempt": op.attempt, "NextAttemptDelaySeconds": delay});
    let event = match (step, ok) {
        (true, true) => (
            "StepSucceeded",
            json!({"Result": payload(result.as_ref()), "RetryDetails": retry}),
        ),
        (true, false) => (
            "StepFailed",
            json!({"Error": event_error(err.as_deref()), "RetryDetails": retry}),
        ),
        (false, true) => (
            "ContextSucceeded",
            json!({"Result": payload(result.as_ref())}),
        ),
        (false, false) => (
            "ContextFailed",
            json!({"Error": event_error(err.as_deref())}),
        ),
    };
    (op.result, op.error) = if ok { (result, None) } else { (None, err) };
    event
}

/// The operations an invocation and `GetDurableExecutionState` are given: every one but the descendants of a context that
/// completed without `ReplayChildren` (checkpoint server, `prune-replay-children`: "The service omits every descendant of a
/// completed CONTEXT whose `ReplayChildren` is not `true`").
pub(crate) fn replayed(ops: &[Op]) -> Vec<&Op> {
    let by_id: std::collections::HashMap<&str, &Op> =
        ops.iter().map(|o| (o.id.as_str(), o)).collect();
    let prunes = |o: &Op| {
        o.kind == "CONTEXT"
            && matches!(o.status.as_str(), "SUCCEEDED" | "FAILED")
            && !o.replay_children
    };
    ops.iter()
        .filter(|op| {
            let mut seen = 0;
            let mut parent = op.parent.as_deref();
            while let Some(p) = parent {
                let Some(up) = by_id.get(p) else { return true };
                if prunes(up) {
                    return false;
                }
                seen += 1;
                if seen > ops.len() {
                    return true;
                }
                parent = up.parent.as_deref();
            }
            true
        })
        .collect()
}
