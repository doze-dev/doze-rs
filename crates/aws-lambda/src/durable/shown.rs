//! How executions, operations and history events are written: as the API answers them (timestamps in epoch seconds, restJson1's
//! default) and as the invocation payload carries them (ISO-8601 strings: the SDK's `normalizeOperation` says the event carries them
//! so).

use aws_core::time::format_iso8601;
use serde_json::{Map, Value, json};

use super::{Execution, Op};

/// How a time is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ts {
    Epoch,
    Iso,
}

impl Ts {
    pub(crate) fn of(self, ms: i64) -> Value {
        match self {
            Self::Epoch => json!(ms as f64 / 1000.0),
            Self::Iso => {
                let iso = format_iso8601(ms.div_euclid(1000));
                json!(iso.replace(".000Z", &format!(".{:03}Z", ms.rem_euclid(1000))))
            }
        }
    }
}

/// Epoch seconds as the API writes a time.
pub(crate) fn secs(ms: i64) -> Value {
    Ts::Epoch.of(ms)
}

/// An `ErrorObject` kept as JSON text, as JSON.
pub(crate) fn error_value(text: Option<&str>) -> Option<Value> {
    text.and_then(|t| serde_json::from_str(t).ok())
}

fn put(o: &mut Map<String, Value>, key: &str, v: Option<Value>) {
    if let Some(v) = v {
        o.insert(key.to_owned(), v);
    }
}

fn text(v: Option<&String>) -> Option<Value> {
    v.map(|s| json!(s))
}

/// An operation as `Operation` in the model.
pub(crate) fn operation(op: &Op, ts: Ts) -> Value {
    let mut o = Map::new();
    o.insert("Id".into(), json!(op.id));
    put(&mut o, "ParentId", text(op.parent.as_ref()));
    put(&mut o, "Name", text(op.name.as_ref()));
    o.insert("Type".into(), json!(op.kind));
    put(&mut o, "SubType", text(op.sub.as_ref()));
    o.insert("StartTimestamp".into(), ts.of(op.start_ms));
    put(&mut o, "EndTimestamp", op.end_ms.map(|m| ts.of(m)));
    o.insert("Status".into(), json!(op.status));
    if let Some((key, details)) = details(op, ts) {
        o.insert(key.into(), details);
    }
    Value::Object(o)
}

fn details(op: &Op, ts: Ts) -> Option<(&'static str, Value)> {
    let mut d = Map::new();
    let outcome = |d: &mut Map<String, Value>| {
        put(d, "Result", text(op.result.as_ref()));
        put(d, "Error", error_value(op.error.as_deref()));
    };
    let key = match op.kind.as_str() {
        "EXECUTION" => {
            put(&mut d, "InputPayload", text(op.input.as_ref()));
            "ExecutionDetails"
        }
        "CONTEXT" => {
            d.insert("ReplayChildren".into(), json!(op.replay_children));
            outcome(&mut d);
            "ContextDetails"
        }
        "STEP" => {
            d.insert("Attempt".into(), json!(op.attempt));
            put(
                &mut d,
                "NextAttemptTimestamp",
                op.next_attempt_ms.map(|m| ts.of(m)),
            );
            outcome(&mut d);
            "StepDetails"
        }
        "WAIT" => {
            put(
                &mut d,
                "ScheduledEndTimestamp",
                op.wait_end_ms.map(|m| ts.of(m)),
            );
            "WaitDetails"
        }
        "CALLBACK" => {
            put(&mut d, "CallbackId", text(op.callback_id.as_ref()));
            outcome(&mut d);
            "CallbackDetails"
        }
        "CHAINED_INVOKE" => {
            outcome(&mut d);
            "ChainedInvokeDetails"
        }
        _ => return None,
    };
    Some((key, Value::Object(d)))
}

/// The `DurableConfig` an execution runs under.
pub(crate) fn config(e: &Execution) -> Value {
    let mut c =
        json!({"ExecutionTimeout": e.timeout_secs, "RetentionPeriodInDays": e.retention_days});
    if let Some(k) = &e.kms_key {
        crate::set(&mut c, "KMSKeyArn", json!(k));
    }
    c
}

/// The qualified ARN of the version an execution runs.
pub(crate) fn function_arn(e: &Execution) -> String {
    e.arn
        .split_once('/')
        .map_or_else(|| e.arn.clone(), |(head, _)| head.to_owned())
}

/// The execution's X-Ray trace header, in X-Ray's format: `Root=1-<start, 8 hex epoch seconds>-<24 hex>;Parent=<16 hex>;Sampled=0`
/// (recorded 2026-11-19: AWS answers one on every `GetDurableExecution`, the same for the life of the execution). The hex digits
/// come from the execution's id and its `EXECUTION` operation's id, two random UUIDs, so the header is fixed without being stored.
pub(crate) fn trace_id(e: &Execution) -> String {
    let hex = |s: &str| -> String { s.chars().filter(char::is_ascii_hexdigit).collect() };
    let pad = |mut h: String, n: usize| {
        while h.len() < n {
            h.push('0');
        }
        h.truncate(n);
        h
    };
    let root = pad(hex(&e.id), 24);
    let parent = pad(hex(e.ops.first().map_or("", |o| o.id.as_str())), 16);
    format!(
        "Root=1-{:08x}-{root};Parent={parent};Sampled=0",
        e.start_ms.div_euclid(1000)
    )
}

/// `GetDurableExecution`'s answer.
pub(crate) fn execution(e: &Execution, data: bool) -> Value {
    let mut o = Map::new();
    o.insert("DurableExecutionArn".into(), json!(e.arn));
    o.insert("DurableExecutionName".into(), json!(e.name));
    o.insert("FunctionArn".into(), json!(function_arn(e)));
    o.insert("Status".into(), json!(e.status));
    o.insert("StartTimestamp".into(), secs(e.start_ms));
    put(&mut o, "EndTimestamp", e.end_ms.map(secs));
    o.insert("Version".into(), json!(e.version));
    o.insert("DurableConfig".into(), config(e));
    o.insert("ExecutionDataIncluded".into(), json!(data));
    o.insert("TraceHeader".into(), json!({"XAmznTraceId": trace_id(e)}));
    if data {
        o.insert("InputPayload".into(), json!(e.input));
        put(&mut o, "Result", text(e.result.as_ref()));
        put(&mut o, "Error", error_value(e.error.as_deref()));
    }
    Value::Object(o)
}

/// An execution as `ListDurableExecutionsByFunction` lists it (`Execution` in the model).
pub(crate) fn summary(e: &Execution) -> Value {
    let mut o = Map::new();
    o.insert("DurableExecutionArn".into(), json!(e.arn));
    o.insert("DurableExecutionName".into(), json!(e.name));
    o.insert("FunctionArn".into(), json!(function_arn(e)));
    o.insert("Status".into(), json!(e.status));
    o.insert("StartTimestamp".into(), secs(e.start_ms));
    put(&mut o, "EndTimestamp", e.end_ms.map(secs));
    put(&mut o, "KMSKeyArn", text(e.kms_key.as_ref()));
    Value::Object(o)
}

/// A payload as an event carries it (`EventInput`, `EventResult`).
pub(crate) fn payload(p: Option<&String>) -> Value {
    p.map_or_else(|| json!({}), |p| json!({"Payload": p}))
}

/// An error as an event carries it (`EventError`).
pub(crate) fn event_error(e: Option<&str>) -> Value {
    error_value(e).map_or_else(|| json!({}), |p| json!({"Payload": p}))
}

/// The members an event's details hold that are execution data (`EventInput`, `EventResult`, `EventError`: a `Payload` and a
/// `Truncated` flag each).
const DATA: &[&str] = &["Input", "Result", "Error"];

/// An event with its execution data withheld when `IncludeExecutionData` is false: the member stays and its `Payload` is replaced by
/// `"Truncated": true` (recorded 2026-11-19 for `ExecutionSucceededDetails.Result`; `Input` and `Error` have the same shape and are
/// treated alike). A member that carried no payload is left as it is.
pub(crate) fn without_data(mut event: Value) -> Value {
    if let Some(o) = event.as_object_mut() {
        for (k, v) in o.iter_mut() {
            if k.ends_with("Details")
                && let Some(d) = v.as_object_mut()
            {
                for key in DATA {
                    if let Some(m) = d.get_mut(*key).and_then(Value::as_object_mut)
                        && m.remove("Payload").is_some()
                    {
                        m.insert("Truncated".into(), json!(true));
                    }
                }
            }
        }
    }
    event
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_seconds_on_the_api_and_an_iso_string_in_the_payload() {
        assert_eq!(Ts::Epoch.of(1_700_000_000_250), json!(1_700_000_000.25));
        assert_eq!(
            Ts::Iso.of(1_700_000_000_250),
            json!("2023-11-14T22:13:20.250Z")
        );
    }

    #[test]
    fn the_trace_header_is_in_xrays_format_and_fixed_for_the_execution() {
        let e = Execution {
            id: "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0".into(),
            start_ms: 1_700_000_000_250,
            ops: vec![Op {
                id: "11223344-5566-7788-99aa-bbccddeeff00".into(),
                kind: "EXECUTION".into(),
                ..Op::default()
            }],
            ..Execution::default()
        };
        assert_eq!(
            trace_id(&e),
            "Root=1-6553f100-0f1e2d3c4b5a69788796a5b4;Parent=1122334455667788;Sampled=0"
        );
        assert_eq!(
            execution(&e, false)["TraceHeader"]["XAmznTraceId"],
            json!(trace_id(&e))
        );
    }

    #[test]
    fn without_data_a_payload_becomes_truncated() {
        let event = json!({"EventType": "ExecutionSucceeded",
            "ExecutionSucceededDetails": {"Result": {"Payload": "42"}},
            "ExecutionStartedDetails": {"Input": {}, "ExecutionTimeout": 600}});
        let v = without_data(event);
        assert_eq!(
            v["ExecutionSucceededDetails"],
            json!({"Result": {"Truncated": true}})
        );
        assert_eq!(
            v["ExecutionStartedDetails"],
            json!({"Input": {}, "ExecutionTimeout": 600})
        );
    }

    #[test]
    fn a_step_shows_its_attempt_and_result() {
        let op = Op {
            id: "a".into(),
            kind: "STEP".into(),
            status: "SUCCEEDED".into(),
            attempt: 1,
            result: Some("1".into()),
            ..Op::default()
        };
        let v = operation(&op, Ts::Epoch);
        assert_eq!(v["StepDetails"], json!({"Attempt": 1, "Result": "1"}));
        assert!(v.get("ParentId").is_none());
    }
}
