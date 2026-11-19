//! The actor's side of an execution's life: starting one (with the idempotency of names), handing an invocation its payload, taking
//! what the invocation answered, and the end of a chained invoke.

use aws_wire::ApiError;
use doze_store::Ack;
use serde_json::{Value, json};

use super::apply::replayed;
use super::shown::{self, Ts};
use super::{
    Execution, INVOCATION_TRIES, Op, RETENTION_DAYS, RUNNING, Wake, closed, error_text, key, token,
};
use crate::core::Core;

/// What the runner asks the actor about executions.
#[derive(Debug)]
pub(crate) enum DurableAsk {
    Start(Box<Start>),
    /// An invocation of the execution is to run: its payload.
    Begin(String),
    Finish(Box<Finish>),
    /// A chained invoke ended: its result, or its `ErrorObject` as JSON text.
    Chained {
        execution: String,
        op: String,
        outcome: Result<Option<String>, String>,
    },
    Tick,
    /// The execution as it is now.
    Peek(String),
}

/// A new execution.
#[derive(Debug)]
pub(crate) struct Start {
    pub(crate) function: String,
    pub(crate) version: String,
    pub(crate) name: Option<String>,
    pub(crate) input: String,
    pub(crate) sync: bool,
    pub(crate) account: String,
    pub(crate) endpoint: String,
    pub(crate) scale: f64,
    /// The function's `DurableConfig`.
    pub(crate) config: Value,
}

/// What an invocation ended with.
#[derive(Debug)]
pub(crate) struct Finish {
    pub(crate) execution: String,
    pub(crate) request_id: String,
    pub(crate) outcome: Outcome,
}

/// The SDK's answer (`Status` `SUCCEEDED`, `FAILED` or `PENDING`), or a failure of the invocation itself.
#[derive(Debug)]
pub(crate) enum Outcome {
    Answered {
        status: String,
        result: Option<String>,
        error: Option<String>,
    },
    Failed(String),
}

/// An invocation to run.
#[derive(Debug)]
pub(crate) struct Begun {
    pub(crate) payload: String,
    pub(crate) function: String,
    pub(crate) version: String,
    pub(crate) account: String,
    pub(crate) endpoint: String,
}

/// What the actor answers.
#[derive(Debug)]
pub(crate) enum DurableReply {
    /// The execution's ARN, and whether it is new.
    Started(Result<(String, bool), ApiError>),
    Begun(Option<Box<Begun>>),
    Execution(Option<Box<Execution>>),
    Done,
}

/// The answer with what it set in motion.
#[derive(Debug)]
pub(crate) struct Answered {
    pub(crate) reply: DurableReply,
    pub(crate) wake: Wake,
    pub(crate) durable: Option<Ack>,
    pub(crate) region: String,
}

impl Core {
    pub(crate) fn durable_ask(&mut self, ask: DurableAsk) -> Answered {
        let reply = match ask {
            DurableAsk::Start(s) => DurableReply::Started(self.start_execution(*s)),
            DurableAsk::Begin(arn) => DurableReply::Begun(self.begin(&arn).map(Box::new)),
            DurableAsk::Finish(f) => {
                self.finish(*f);
                DurableReply::Done
            }
            DurableAsk::Chained {
                execution,
                op,
                outcome,
            } => {
                self.chained_done(&execution, &op, outcome);
                DurableReply::Done
            }
            DurableAsk::Tick => {
                self.durable_tick();
                DurableReply::Done
            }
            DurableAsk::Peek(arn) => {
                DurableReply::Execution(self.execution(&arn).ok().cloned().map(Box::new))
            }
        };
        let durable = if self.live.changed {
            self.flush().ok().flatten()
        } else {
            None
        };
        Answered {
            reply,
            wake: self.take_wake(),
            durable,
            region: self.region.clone(),
        }
    }

    /// What the call just handled set in motion, with the next deadline when durable state changed.
    pub(crate) fn take_wake(&mut self) -> Wake {
        let mut wake = std::mem::take(&mut self.live.wake);
        if std::mem::take(&mut self.live.changed) {
            wake.due = Some(self.durable_due());
        }
        wake
    }

    /// An execution of the same function already started under the name: the same one when the payload is the same (developer guide,
    /// "Idempotency"), refused when it is not.
    fn named(&self, s: &Start) -> Result<Option<String>, ApiError> {
        let Some(name) = &s.name else { return Ok(None) };
        let prefix = format!("{}/", s.function);
        let Some((_, e)) = self
            .durable
            .with_prefix(&prefix)
            .find(|(_, e)| &e.name == name)
        else {
            return Ok(None);
        };
        // Recorded 2026-11-19: a name already started answers with that execution, whatever the payload (the model's
        // DurableExecutionAlreadyStartedException was not given for a different payload).
        Ok(Some(e.arn.clone()))
    }

    fn start_execution(&mut self, s: Start) -> Result<(String, bool), ApiError> {
        if let Some(arn) = self.named(&s)? {
            return Ok((arn, false));
        }
        let now = self.now_ms();
        let id = crate::functions::uuid()?;
        // Memory: with no name AWS generates one ("a system-generated unique identifier"); here it is the execution's id.
        let name = s.name.clone().unwrap_or_else(|| id.clone());
        let arn = super::arn(
            &self.region,
            &s.account,
            &s.function,
            &s.version,
            &name,
            &id,
        );
        let k = key(&s.function, &id);
        let num = |key: &str| s.config.get(key).and_then(Value::as_i64);
        let mut e = Execution {
            arn: arn.clone(),
            name,
            id: id.clone(),
            function: s.function,
            version: s.version,
            account: s.account,
            endpoint: s.endpoint,
            status: RUNNING.to_owned(),
            start_ms: now,
            input: s.input.clone(),
            timeout_secs: num("ExecutionTimeout").unwrap_or(super::SYNC_LIMIT_SECS),
            retention_days: num("RetentionPeriodInDays").unwrap_or(RETENTION_DAYS),
            kms_key: s
                .config
                .get("KMSKeyArn")
                .and_then(Value::as_str)
                .map(str::to_owned),
            sync: s.sync,
            scale: s.scale,
            token: token(&k, 0),
            ..Execution::default()
        };
        let op = Op {
            id: crate::functions::uuid()?,
            kind: "EXECUTION".to_owned(),
            status: "STARTED".to_owned(),
            start_ms: now,
            input: Some(s.input.clone()),
            ..Op::default()
        };
        let timeout = e.timeout_secs;
        e.event(
            now,
            "ExecutionStarted",
            Some(&op),
            json!({"Input": {"Payload": s.input}, "ExecutionTimeout": timeout}),
        );
        e.ops.push(op);
        self.durable.insert(k.clone(), e);
        self.wake_up(&k);
        Ok((arn, true))
    }

    /// The payload of the next invocation (the SDK's `DurableExecutionInvocationInput`), unless one is running or the execution ended.
    fn begin(&mut self, arn: &str) -> Option<Begun> {
        let k = super::key_of(arn)?;
        if self.live.busy.contains(&k) {
            return None;
        }
        let now = self.now_ms();
        let e = self.durable.get_mut(&k).filter(|e| e.status == RUNNING)?;
        e.seq += 1;
        e.token = token(&k, e.seq);
        e.invocation = Some((now, String::new()));
        let updated: Vec<String> = std::mem::take(&mut e.dirty);
        let ops: Vec<Value> = replayed(&e.ops)
            .into_iter()
            .map(|o| shown::operation(o, Ts::Iso))
            .collect();
        let payload = json!({
            "DurableExecutionArn": e.arn,
            "CheckpointToken": e.token,
            "UpdatedOperationIds": updated,
            "InitialExecutionState": {"Operations": ops, "NextMarker": ""},
        });
        let begun = Begun {
            payload: payload.to_string(),
            function: e.function.clone(),
            version: e.version.clone(),
            account: e.account.clone(),
            endpoint: e.endpoint.clone(),
        };
        self.live.busy.insert(k);
        self.live.changed = true;
        Some(begun)
    }

    fn finish(&mut self, f: Finish) {
        let Some(k) = super::key_of(&f.execution) else {
            return;
        };
        self.live.busy.remove(&k);
        self.live.changed = true;
        let now = self.now_ms();
        let Some(e) = self.durable.get_mut(&k) else {
            return;
        };
        let started = e.invocation.take().map_or(now, |(at, _)| at);
        let mut details = json!({"StartTimestamp": shown::secs(started), "EndTimestamp": shown::secs(now), "RequestId": f.request_id});
        if let Outcome::Failed(err)
        | Outcome::Answered {
            error: Some(err), ..
        } = &f.outcome
        {
            crate::set(&mut details, "Error", shown::event_error(Some(err)));
        }
        e.event(now, "InvocationCompleted", None, details);
        if closed(&e.status) {
            return;
        }
        let arn = e.arn.clone();
        match f.outcome {
            Outcome::Answered {
                status,
                result,
                error,
            } => {
                e.failures = 0;
                match status.as_str() {
                    "SUCCEEDED" => {
                        // An empty result after the SDK checkpointed a large one: the execution already ended with it.
                        e.close(now, "SUCCEEDED", result, None);
                    }
                    "FAILED" => e.close(now, "FAILED", None, error),
                    _ if !waiting(e) => e.close(
                        now,
                        "FAILED",
                        None,
                        // The words of AWS's local test runner for this case.
                        Some(error_text(
                            "InvalidParameterValueException",
                            "Cannot return PENDING status with no pending operations.",
                        )),
                    ),
                    _ => e.wake = !e.dirty.is_empty(),
                }
            }
            Outcome::Failed(err) => {
                e.failures += 1;
                if e.failures >= INVOCATION_TRIES {
                    e.close(now, "FAILED", None, Some(err));
                } else {
                    e.wake = false;
                    e.retry_at = Some(now + 1000 * i64::from(e.failures));
                }
            }
        }
        if closed(&e.status) {
            self.live.wake.closed.push(arn);
        } else if e.wake {
            self.wake_up(&k);
        }
    }

    fn chained_done(
        &mut self,
        execution: &str,
        op_id: &str,
        outcome: Result<Option<String>, String>,
    ) {
        let now = self.now_ms();
        let Ok(e) = self.execution_mut(execution) else {
            return;
        };
        if closed(&e.status) {
            return;
        }
        let Some(op) = e.op_mut(op_id).filter(|o| o.status == "STARTED") else {
            return;
        };
        op.end_ms = Some(now);
        let (kind, details) = match outcome {
            Ok(result) => {
                op.status = "SUCCEEDED".into();
                op.result = result;
                (
                    "ChainedInvokeSucceeded",
                    json!({"Result": shown::payload(op.result.as_ref())}),
                )
            }
            Err(err) => {
                op.status = "FAILED".into();
                op.error = Some(err);
                (
                    "ChainedInvokeFailed",
                    json!({"Error": shown::event_error(op.error.as_deref())}),
                )
            }
        };
        let op = op.clone();
        e.event(now, kind, Some(&op), details);
        e.mark(op_id);
        let k = key(&e.function, &e.id);
        self.wake_up(&k);
    }
}

/// Whether an execution has something to wait for: a wait, a callback or a chained invoke under way, a step to retry, or a change
/// the SDK has not seen.
fn waiting(e: &Execution) -> bool {
    !e.dirty.is_empty()
        || e.ops.iter().any(|o| match o.kind.as_str() {
            "WAIT" | "CALLBACK" | "CHAINED_INVOKE" => o.status == "STARTED",
            "STEP" => matches!(o.status.as_str(), "PENDING" | "READY"),
            _ => false,
        })
}
