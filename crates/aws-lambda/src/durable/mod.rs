//! Durable functions: an execution that spans many invocations of one function version. The function's SDK
//! (`@aws/durable-execution-sdk-js`, `aws-durable-execution-sdk-python`) records each step, wait, callback, child context and chained
//! invoke with `CheckpointDurableExecution`; when the invocation has nothing left to do but wait it answers `PENDING`, and Lambda
//! invokes the function again when a wait ends, a step's retry is due, a callback is answered or a chained invoke returns. The SDK
//! replays from the operations it is handed.
//!
//! The references are the model (`models/lambda.json`: shapes, enums, limits, documentation), AWS's developer guide (qualified
//! invokes, idempotent names, `DurableConfig`), and AWS's own local stand-in for the service, the checkpoint server in
//! `@aws/durable-execution-sdk-js-testing` 1.3.2 (`dist/checkpoint-server`), whose checks say they were measured against the service:
//! the status transitions, the validation messages, `UpdatedOperationIds`, the pruning of completed contexts' children. What the
//! conformance recording of 2026-11-19 settled says `Recorded 2026-11-19`; a message from that server is cited as such, one from
//! none of these sources is marked `(memory)`.
//!
//! The state is one record per execution in `lambda.redb` (`durable`). Timers (wait ends, retries, callback timeouts, the execution
//! timeout, retention) are one [`doze_runtime::Deadlines`] task per region that sleeps until the soonest and is gone while nothing is
//! due. Invocations run beside the actor (`run.rs`), one at a time per execution.

mod answer;
mod apply;
mod check;
pub(crate) mod life;
pub(crate) mod ops;
pub(crate) mod run;
mod shown;
mod stored;
mod timers;

use std::collections::HashSet;

use aws_core::base64;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(crate) const TABLE: &str = "durable";
pub(crate) const WHAT: &str = "durable execution";

pub(crate) const RUNNING: &str = "RUNNING";
/// The default history retention (developer guide, "Configure Lambda durable functions": "Default: 14").
pub(crate) const RETENTION_DAYS: i64 = 14;
/// A synchronous invocation waits at most this long for its execution (developer guide: "limited to 15 minutes").
pub(crate) const SYNC_LIMIT_SECS: i64 = 900;
/// How many times an invocation that fails without an answer from the SDK (a crash, a timeout) is tried before the execution
/// fails (memory: Lambda retries such invocations; the count and the delays are doze's).
pub(crate) const INVOCATION_TRIES: u32 = 3;

/// One durable execution.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Execution {
    pub(crate) arn: String,
    pub(crate) name: String,
    pub(crate) id: String,
    pub(crate) function: String,
    /// `$LATEST` or a version number: the version every invocation of the execution runs.
    pub(crate) version: String,
    pub(crate) account: String,
    /// Where the function's SDK reaches this server, as the invoking call saw it.
    pub(crate) endpoint: String,
    pub(crate) status: String,
    pub(crate) start_ms: i64,
    pub(crate) end_ms: Option<i64>,
    pub(crate) input: String,
    pub(crate) result: Option<String>,
    /// The `ErrorObject` as JSON text.
    pub(crate) error: Option<String>,
    pub(crate) timeout_secs: i64,
    pub(crate) retention_days: i64,
    pub(crate) kms_key: Option<String>,
    pub(crate) sync: bool,
    /// What every wait and retry delay is multiplied by (1 is AWS; `DOZE_LAMBDA_DURABLE_WAIT_SCALE`).
    pub(crate) scale: f64,
    pub(crate) ops: Vec<Op>,
    /// The history, each event as JSON text; an event's `EventId` is its place plus one.
    pub(crate) events: Vec<String>,
    /// The operations changed since the SDK last saw them (a checkpoint's answer or an invocation's `UpdatedOperationIds`).
    pub(crate) dirty: Vec<String>,
    pub(crate) token: String,
    pub(crate) seq: u64,
    /// An invocation should run (cleared when one ends with nothing new to see).
    pub(crate) wake: bool,
    /// When an invocation that failed is tried again.
    pub(crate) retry_at: Option<i64>,
    pub(crate) failures: u32,
    /// When the running invocation began, and its request id.
    pub(crate) invocation: Option<(i64, String)>,
}

/// One operation of an execution.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Op {
    pub(crate) id: String,
    pub(crate) parent: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) kind: String,
    pub(crate) sub: Option<String>,
    pub(crate) status: String,
    pub(crate) start_ms: i64,
    pub(crate) end_ms: Option<i64>,
    pub(crate) result: Option<String>,
    /// The `ErrorObject` as JSON text.
    pub(crate) error: Option<String>,
    /// `EXECUTION`: the input; `CHAINED_INVOKE`: what the function is invoked with.
    pub(crate) input: Option<String>,
    pub(crate) replay_children: bool,
    pub(crate) attempt: i64,
    pub(crate) next_attempt_ms: Option<i64>,
    pub(crate) wait_secs: Option<i64>,
    pub(crate) wait_end_ms: Option<i64>,
    pub(crate) callback_id: Option<String>,
    pub(crate) callback_timeout: i64,
    pub(crate) callback_heartbeat: i64,
    pub(crate) callback_deadline_ms: Option<i64>,
    pub(crate) heartbeat_deadline_ms: Option<i64>,
    /// `CHAINED_INVOKE`: the function invoked.
    pub(crate) target: Option<String>,
}

/// What the runner is to do after the actor answered: invocations to start, chained invokes to make, executions that closed, and
/// the region's next deadline (`Some` only when durable state changed).
#[derive(Debug, Default)]
pub(crate) struct Wake {
    pub(crate) runs: Vec<String>,
    pub(crate) chained: Vec<Chained>,
    pub(crate) closed: Vec<String>,
    pub(crate) due: Option<Option<i64>>,
}

impl Wake {
    pub(crate) fn is_empty(&self) -> bool {
        self.runs.is_empty()
            && self.chained.is_empty()
            && self.closed.is_empty()
            && self.due.is_none()
    }
}

/// A chained invoke to make for an execution.
#[derive(Clone, Debug)]
pub(crate) struct Chained {
    pub(crate) execution: String,
    pub(crate) op: String,
    pub(crate) function: String,
    pub(crate) payload: String,
    pub(crate) account: String,
    pub(crate) endpoint: String,
}

/// What the actor keeps for durable executions beside the records: the executions with an invocation running, and what the call
/// being handled set in motion.
#[derive(Debug, Default)]
pub(crate) struct Live {
    pub(crate) busy: HashSet<String>,
    pub(crate) wake: Wake,
    pub(crate) changed: bool,
    /// Whether a tick has run since the region opened (the first one restarts what was in flight).
    pub(crate) ticked: bool,
}

/// The record key of an execution: `<function>/<id>`.
pub(crate) fn key(function: &str, id: &str) -> String {
    format!("{function}/{id}")
}

/// `arn:aws:lambda:<region>:<account>:function:<name>:<version>/durable-execution/<execution name>/<id>` (the model's pattern).
pub(crate) fn arn(
    region: &str,
    account: &str,
    function: &str,
    version: &str,
    name: &str,
    id: &str,
) -> String {
    format!(
        "arn:aws:lambda:{region}:{account}:function:{function}:{version}/durable-execution/{name}/{id}"
    )
}

/// The record key an execution ARN names, if it is one.
pub(crate) fn key_of(arn: &str) -> Option<String> {
    let (head, tail) = arn.split_once("/durable-execution/")?;
    let function = head.split(':').nth(6)?;
    let (_, id) = tail.split_once('/')?;
    (!function.is_empty() && !id.is_empty() && !id.contains('/')).then(|| key(function, id))
}

/// A token in the model's alphabet (`^[A-Za-z0-9+/]+={0,2}$`): the execution's key, a sequence number and random bytes.
pub(crate) fn token(key: &str, seq: u64) -> String {
    let salt = aws_seal::alphanumeric(16).unwrap_or_default();
    base64::encode(format!("{key}|{seq}|{salt}").as_bytes())
}

/// The execution key a checkpoint token was made for.
pub(crate) fn token_key(token: &str) -> Option<String> {
    let bytes = base64::decode(token)?;
    let text = String::from_utf8(bytes).ok()?;
    let mut parts = text.split('|');
    let key = parts.next()?;
    parts.next()?.parse::<u64>().ok()?;
    Some(key.to_owned())
}

/// A callback id: the execution's ARN, the operation and random bytes, base64 (the model's alphabet).
pub(crate) fn callback_id(execution: &str, op: &str) -> String {
    let salt = aws_seal::alphanumeric(16).unwrap_or_default();
    base64::encode(
        json!({"e": execution, "o": op, "t": salt})
            .to_string()
            .as_bytes(),
    )
}

/// The execution ARN and operation id a callback id names.
pub(crate) fn callback_target(id: &str) -> Option<(String, String)> {
    let v: Value = serde_json::from_slice(&base64::decode(id)?).ok()?;
    Some((
        v.get("e")?.as_str()?.to_owned(),
        v.get("o")?.as_str()?.to_owned(),
    ))
}

/// Whether a status is one an execution ends in.
pub(crate) fn closed(status: &str) -> bool {
    status != RUNNING
}

/// The `ErrorObject` a message makes.
pub(crate) fn error_text(kind: &str, message: &str) -> String {
    json!({"ErrorType": kind, "ErrorMessage": message}).to_string()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn an_arn_a_token_and_a_callback_id_lead_back_to_their_execution() {
        let a = arn("us-east-1", "111122223333", "f", "$LATEST", "run-1", "abc");
        assert_eq!(key_of(&a).as_deref(), Some("f/abc"));
        assert_eq!(key_of("arn:aws:lambda:us-east-1:1:function:f"), None);
        let t = token("f/abc", 3);
        assert!(
            t.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
        );
        assert_eq!(token_key(&t).as_deref(), Some("f/abc"));
        let c = callback_id(&a, "op1");
        assert_eq!(callback_target(&c), Some((a, "op1".to_owned())));
        assert_eq!(callback_target("bm90IGpzb24="), None);
    }
}

#[cfg(test)]
mod timeline;
