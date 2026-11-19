//! The durable execution operations of the API: the SDK's two (`CheckpointDurableExecution`, `GetDurableExecutionState`), the
//! callbacks an outside system answers, and reading and stopping executions.

use aws_wire::ApiError;
use serde_json::{Value, json};

use super::apply::replayed;
use super::shown::{self, Ts, secs};
use super::{Execution, RUNNING, closed, key_of, token, token_key};
use crate::core::{Core, Out};
use crate::errors::{invalid, no_function, not_found};
use crate::req::Req;

/// The page size when `MaxItems` is 0 or absent (the model: "The default is 100").
const PAGE: usize = 100;

/// `ResourceNotFoundException` for an execution. AWS does not name the ARN.
// Recorded 2026-11-19.
fn no_execution() -> ApiError {
    not_found("Durable Execution does not exist")
}

/// A token that is not the execution's current one (memory: the checkpoint server says "Invalid checkpoint token").
fn bad_token() -> ApiError {
    invalid("Invalid checkpoint token")
}

/// `CallbackTimeoutException` for a callback that timed out or is already answered (AWS's words, not the model documentation's).
// Recorded 2026-11-19.
fn callback_closed() -> ApiError {
    ApiError::new(
        400,
        "CallbackTimeoutException",
        "The callback is either timed out or already completed",
    )
}

fn page_size(r: &Req) -> usize {
    r.int("MaxItems")
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n > 0)
        .unwrap_or(PAGE)
}

/// One page of `items` from `Marker` (the index to start at).
fn page(r: &Req, items: Vec<Value>) -> (Vec<Value>, Option<String>) {
    let from = r
        .opt("Marker")
        .and_then(|m| m.parse::<usize>().ok())
        .unwrap_or(0);
    let size = page_size(r);
    let next = (from + size < items.len()).then(|| (from + size).to_string());
    (items.into_iter().skip(from).take(size).collect(), next)
}

impl Core {
    pub(crate) fn execution(&self, arn: &str) -> Result<&Execution, ApiError> {
        key_of(arn)
            .and_then(|k| self.durable.get(&k))
            .filter(|e| e.arn == arn)
            .ok_or_else(no_execution)
    }

    pub(crate) fn execution_mut(&mut self, arn: &str) -> Result<&mut Execution, ApiError> {
        self.live.changed = true;
        key_of(arn)
            .and_then(|k| self.durable.get_mut(&k))
            .filter(|e| e.arn == arn)
            .ok_or_else(no_execution)
    }

    pub(crate) fn get_durable_execution(&self, r: &Req) -> Result<Out, ApiError> {
        let e = self.execution(r.text("DurableExecutionArn"))?;
        Ok(shown::execution(e, r.flag("IncludeExecutionData").unwrap_or(true)).into())
    }

    pub(crate) fn get_durable_history(&self, r: &Req) -> Result<Out, ApiError> {
        let e = self.execution(r.text("DurableExecutionArn"))?;
        let data = r.flag("IncludeExecutionData").unwrap_or(true);
        let mut events: Vec<Value> = e
            .events
            .iter()
            .filter_map(|t| serde_json::from_str(t).ok())
            .map(|ev| if data { ev } else { shown::without_data(ev) })
            .collect();
        if r.flag("ReverseOrder") == Some(true) {
            events.reverse();
        }
        let (events, next) = page(r, events);
        let mut out = json!({"Events": events});
        if let Some(n) = next {
            crate::set(&mut out, "NextMarker", json!(n));
        }
        Ok(out.into())
    }

    pub(crate) fn get_durable_state(&self, r: &Req) -> Result<Out, ApiError> {
        let arn = r.text("DurableExecutionArn");
        let e = self.execution(arn)?;
        if token_key(r.text("CheckpointToken")) != key_of(arn) {
            return Err(bad_token());
        }
        let ops: Vec<Value> = replayed(&e.ops)
            .into_iter()
            .map(|o| shown::operation(o, Ts::Epoch))
            .collect();
        let (ops, next) = page(r, ops);
        let mut out = json!({"Operations": ops});
        if let Some(n) = next {
            crate::set(&mut out, "NextMarker", json!(n));
        }
        Ok(out.into())
    }

    pub(crate) fn list_durable_executions(&self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self
            .functions
            .get(&f.name)
            .ok_or_else(|| no_function(&self.arn(r, &f.name, None)))?;
        let version = f.qualifier.as_deref().map(|q| {
            function
                .aliases
                .iter()
                .find(|a| a.name == q)
                .map_or_else(|| q.to_owned(), |a| a.version.clone())
        });
        let statuses: Vec<&str> = r
            .value("Statuses")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let (after, before) = (
            time_of(r.value("StartedAfter")),
            time_of(r.value("StartedBefore")),
        );
        let prefix = format!("{}/", f.name);
        let mut found: Vec<&Execution> = self
            .durable
            .with_prefix(&prefix)
            .map(|(_, e)| e)
            .filter(|e| version.as_deref().is_none_or(|v| e.version == v))
            .filter(|e| r.opt("DurableExecutionName").is_none_or(|n| e.name == n))
            .filter(|e| statuses.is_empty() || statuses.contains(&e.status.as_str()))
            .filter(|e| {
                after.is_none_or(|t| e.start_ms > t) && before.is_none_or(|t| e.start_ms < t)
            })
            .collect();
        // The model: "ReverseOrder: Set to true to return results in chronological order (oldest first). Default is false."
        found.sort_by_key(|e| (e.start_ms, e.id.clone()));
        if r.flag("ReverseOrder") != Some(true) {
            found.reverse();
        }
        let (items, next) = page(r, found.into_iter().map(shown::summary).collect());
        let mut out = json!({"DurableExecutions": items});
        if let Some(n) = next {
            crate::set(&mut out, "NextMarker", json!(n));
        }
        Ok(out.into())
    }

    pub(crate) fn stop_durable_execution(&mut self, r: &Req) -> Result<Out, ApiError> {
        let now = self.now_ms();
        let err = r.value("Error").map(Value::to_string);
        let e = self.execution_mut(r.text("DurableExecutionArn"))?;
        // Recorded 2026-11-19: stopping a stopped execution again is answered with the time it stopped.
        if e.status == "STOPPED"
            && let Some(at) = e.end_ms
        {
            return Ok(json!({"StopTimestamp": secs(at)}).into());
        }
        if closed(&e.status) {
            // Memory: AWS's answer for an execution that succeeded or failed is not recorded.
            return Err(invalid(format!(
                "The durable execution is not running: it is {}.",
                e.status
            )));
        }
        e.close(now, "STOPPED", None, err);
        let arn = e.arn.clone();
        self.live.wake.closed.push(arn);
        Ok(json!({"StopTimestamp": secs(now)}).into())
    }

    pub(crate) fn checkpoint_durable(&mut self, r: &Req) -> Result<Out, ApiError> {
        let now = self.now_ms();
        let updates: Vec<Value> = r
            .value("Updates")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let e = self.execution_mut(r.text("DurableExecutionArn"))?;
        if e.status != RUNNING || r.text("CheckpointToken") != e.token {
            return Err(bad_token());
        }
        super::check::updates(&updates, &e.ops, e.sync)?;
        let mut chained = Vec::new();
        for u in &updates {
            chained.extend(e.apply(u, now));
        }
        let seen = std::mem::take(&mut e.dirty);
        let changed: Vec<Value> = seen
            .iter()
            .filter_map(|id| e.ops.iter().find(|o| &o.id == id))
            .map(|o| shown::operation(o, Ts::Epoch))
            .collect();
        let mut out = json!({"NewExecutionState": {"Operations": changed}});
        let mut ended = None;
        if e.status == RUNNING {
            e.seq += 1;
            e.token = token(&super::key(&e.function, &e.id), e.seq);
            crate::set(&mut out, "CheckpointToken", json!(e.token));
        } else {
            // The SDK logs "No CheckpointToken after the execution's terminal update": none is given once it has ended.
            ended = Some(e.arn.clone());
        }
        self.live.wake.closed.extend(ended);
        self.live.wake.chained.extend(chained);
        Ok(out.into())
    }

    /// `SendDurableExecutionCallbackSuccess`, `...Failure` and `...Heartbeat`.
    pub(crate) fn callback(&mut self, r: &Req, outcome: Callback) -> Result<Out, ApiError> {
        let now = self.now_ms();
        let id = r.text("CallbackId");
        // Memory: what AWS answers for a callback id it never issued is not recorded; it is taken as not found.
        let (arn, op_id) = super::callback_target(id)
            .ok_or_else(|| not_found(format!("Callback not found: {id}")))?;
        let e = self
            .execution_mut(&arn)
            .map_err(|_| not_found(format!("Callback not found: {id}")))?;
        let running = e.status == RUNNING;
        let op = e
            .op_mut(&op_id)
            .filter(|o| o.callback_id.as_deref() == Some(id))
            .ok_or_else(|| not_found(format!("Callback not found: {id}")))?;
        if !running || op.status != "STARTED" {
            return Err(callback_closed());
        }
        match outcome {
            Callback::Heartbeat => {
                if op.callback_heartbeat > 0 {
                    op.heartbeat_deadline_ms = Some(now + op.callback_heartbeat * 1000);
                }
            }
            Callback::Success(result) => {
                op.result = result;
                e.complete_callback(&op_id, "SUCCEEDED", now);
            }
            Callback::Failure(err) => {
                op.error = Some(err);
                e.complete_callback(&op_id, "FAILED", now);
            }
        }
        let wakes = e.wake;
        let key = super::key(&e.function, &e.id);
        if wakes {
            self.wake_up(&key);
        }
        Ok(json!({}).into())
    }

    /// Asks for an invocation of the execution, unless one is running (it sees what changed in its next checkpoint's answer, or is
    /// followed by another when it ends).
    pub(crate) fn wake_up(&mut self, key: &str) {
        if let Some(e) = self.durable.get_mut(key) {
            e.wake = true;
            if e.status == RUNNING
                && !self.live.busy.contains(key)
                && !self.live.wake.runs.iter().any(|a| a == &e.arn)
            {
                self.live.wake.runs.push(e.arn.clone());
            }
        }
        self.live.changed = true;
    }
}

/// How a callback is answered.
#[derive(Debug)]
pub(crate) enum Callback {
    Success(Option<String>),
    /// The `ErrorObject` as JSON text.
    Failure(String),
    Heartbeat,
}

impl Execution {
    /// Closes a callback that was answered or timed out, with its event, and asks for an invocation.
    pub(crate) fn complete_callback(&mut self, op_id: &str, status: &str, now: i64) {
        let Some(op) = self.op_mut(op_id) else { return };
        op.status = status.to_owned();
        op.end_ms = Some(now);
        op.callback_deadline_ms = None;
        op.heartbeat_deadline_ms = None;
        let (kind, details) = match status {
            "SUCCEEDED" => (
                "CallbackSucceeded",
                json!({"Result": shown::payload(op.result.as_ref())}),
            ),
            "TIMED_OUT" => (
                "CallbackTimedOut",
                json!({"Error": shown::event_error(op.error.as_deref())}),
            ),
            _ => (
                "CallbackFailed",
                json!({"Error": shown::event_error(op.error.as_deref())}),
            ),
        };
        let op = op.clone();
        self.event(now, kind, Some(&op), details);
        self.mark(op_id);
        self.wake = true;
    }
}

/// A time given as a query parameter: ISO 8601 (what the SDKs send) or epoch seconds.
fn time_of(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::String(s) => aws_core::time::parse_rfc3339(s)
            .map(|t| t * 1000)
            .or_else(|| s.parse::<f64>().ok().map(|f| (f * 1000.0) as i64)),
        Value::Number(n) => n.as_f64().map(|f| (f * 1000.0) as i64),
        _ => None,
    }
}
