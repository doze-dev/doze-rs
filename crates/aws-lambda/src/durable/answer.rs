//! What an invocation of a durable function answered, read as the SDK's `DurableExecutionInvocationOutput`, and the answer of a
//! synchronous invoke made from the execution it ran.

use aws_core::base64;
use aws_wire::ApiError;
use aws_wire::service::Caller;
use serde_json::{Value, json};

use super::life::Outcome;
use super::{Execution, SYNC_LIMIT_SECS, error_text};
use crate::invoke::RunOut;
use crate::resolve::Resolved;

/// The caller an execution's invocations run as: Lambda itself, for the account that started it.
pub(crate) fn lambda_caller(region: &str, account: &str, endpoint: &str) -> Caller {
    Caller {
        access_key_id: "test".to_owned(),
        account_id: account.to_owned(),
        region: region.to_owned(),
        endpoint: endpoint.to_owned(),
        service: Some("lambda".to_owned()),
        session_token: None,
    }
}

/// How long a synchronous invoke waits (developer guide: "Synchronous invocations are limited to 15 minutes, or less if the
/// function timeout or execution timeout is lower").
pub(crate) fn sync_limit(config: &Value, rs: &Resolved) -> i64 {
    let execution = config
        .get("ExecutionTimeout")
        .and_then(Value::as_i64)
        .unwrap_or(SYNC_LIMIT_SECS);
    SYNC_LIMIT_SECS.min(execution).min(timeout_of(rs))
}

fn timeout_of(rs: &Resolved) -> i64 {
    rs.config
        .get("Timeout")
        .and_then(Value::as_i64)
        .unwrap_or(3)
}

/// What an invocation's run says: the SDK's answer, or why there is none.
pub(crate) fn outcome_of(ran: Result<RunOut, ApiError>) -> (String, Outcome) {
    let out = match ran {
        Ok(out) => out,
        Err(e) => {
            return (
                String::new(),
                Outcome::Failed(error_text(&e.code, &e.message)),
            );
        }
    };
    if out.error {
        return (out.request_id, Outcome::Failed(error_object(&out.payload)));
    }
    let answer: Value = serde_json::from_slice(&out.payload).unwrap_or(Value::Null);
    let status = answer
        .get("Status")
        .and_then(Value::as_str)
        .filter(|s| matches!(*s, "SUCCEEDED" | "FAILED" | "PENDING"));
    let Some(status) = status else {
        // Memory: AWS's words for a durable function that does not answer with a Status are not recorded.
        let err = error_text(
            "InvalidResponse",
            "The function did not answer as a durable function: wrap the handler with the durable execution SDK (withDurableExecution, @durable_execution).",
        );
        return (out.request_id, Outcome::Failed(err));
    };
    let outcome = Outcome::Answered {
        status: status.to_owned(),
        result: answer
            .get("Result")
            .and_then(Value::as_str)
            .map(str::to_owned),
        error: answer
            .get("Error")
            .filter(|e| !e.is_null())
            .map(Value::to_string),
    };
    (out.request_id, outcome)
}

/// A function error document (`errorType`, `errorMessage`, `stackTrace`) as an `ErrorObject`.
pub(crate) fn error_object(payload: &[u8]) -> String {
    let doc: Value = serde_json::from_slice(payload).unwrap_or(Value::Null);
    let mut e = json!({
        "ErrorType": doc.get("errorType").and_then(Value::as_str).unwrap_or("Unhandled"),
        "ErrorMessage": doc.get("errorMessage").and_then(Value::as_str).unwrap_or("The invocation failed."),
    });
    if let Some(trace) = doc.get("stackTrace").and_then(Value::as_array) {
        crate::set(
            &mut e,
            "StackTrace",
            json!(
                trace
                    .iter()
                    .map(|l| l.as_str().map_or_else(|| l.to_string(), str::to_owned))
                    .collect::<Vec<_>>()
            ),
        );
    }
    e.to_string()
}

/// The answer of a synchronous invoke: the execution's result, or its error as a function error (memory: the document's members
/// follow a function error's), or a timeout when it did not end in time.
pub(crate) fn sync_reply(arn: &str, version: &str, ended: Option<&Execution>, limit: i64) -> Value {
    let mut reply =
        json!({"StatusCode": 200, "ExecutedVersion": version, "DurableExecutionArn": arn});
    let body = match ended {
        Some(e) if e.status == "SUCCEEDED" => e.result.clone().unwrap_or_else(|| "null".to_owned()),
        Some(e) => {
            crate::set(&mut reply, "FunctionError", json!("Unhandled"));
            let err: Value = e
                .error
                .as_deref()
                .and_then(|t| serde_json::from_str(t).ok())
                .unwrap_or(Value::Null);
            let mut doc = json!({
                "errorType": err.get("ErrorType").cloned().unwrap_or_else(|| json!(e.status)),
                "errorMessage": err.get("ErrorMessage").cloned().unwrap_or_else(|| json!(format!("The durable execution ended {}.", e.status))),
            });
            if let Some(trace) = err.get("StackTrace") {
                crate::set(&mut doc, "stackTrace", trace.clone());
            }
            doc.to_string()
        }
        None => {
            crate::set(&mut reply, "FunctionError", json!("Unhandled"));
            json!({
                "errorType": "Sandbox.Timedout",
                "errorMessage": format!("The synchronous invocation waited {limit} seconds and the durable execution has not ended; it goes on (GetDurableExecution {arn})."),
            })
            .to_string()
        }
    };
    crate::set(
        &mut reply,
        "Payload",
        json!(base64::encode(body.as_bytes())),
    );
    reply
}
