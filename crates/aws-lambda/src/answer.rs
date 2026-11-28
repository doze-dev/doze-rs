//! What an invocation is made of and answers with: the payload of a request, the documents of a function error, and the reply of a
//! synchronous invoke.

use std::time::Duration;

use aws_core::base64;
use aws_wire::ApiError;
use aws_wire::rest::RestReply;
use aws_wire::service::Call;
use bytes::Bytes;
use serde_json::{Map, Value, json};

use crate::errors::invalid_content;
use crate::invoke::RunOut;
use crate::req::Req;
use crate::rt::InitFailure;

/// Jackson's complaint about a payload that is not JSON, as AWS words it (recorded 2026-11-09 for a key that is not quoted).
pub(crate) fn parse_failure(payload: &[u8]) -> ApiError {
    let text = String::from_utf8_lossy(payload);
    let detail = match serde_json::from_str::<Value>(&text) {
        Ok(_) => return invalid_content("Could not parse request body into json."),
        Err(e) => {
            let first = text.trim_start().chars().next();
            let bad = text
                .chars()
                .nth(e.column().saturating_sub(1))
                .unwrap_or('?');
            if first == Some('{') && bad != '"' {
                format!(
                    "Unexpected character ('{bad}' (code {})): was expecting double-quote to start field name\n at [Source: REDACTED (`StreamReadFeature.INCLUDE_SOURCE_IN_LOCATION` disabled); line: {}, column: {}]",
                    u32::from(bad),
                    e.line(),
                    e.column()
                )
            } else {
                format!("{e}")
            }
        }
    };
    invalid_content(format!(
        "Could not parse request body into json: Could not parse payload into json: {detail}"
    ))
}

/// The payload of an invoke request: absent is `{}`; what is there must be JSON. The bytes are the decoded tree's (no base64 here),
/// and they are only checked, not built into a value.
pub(crate) fn payload_of(r: &Req) -> Result<Bytes, ApiError> {
    let Some(bytes) = r.payload.clone().filter(|b| !b.is_empty()) else {
        return Ok(Bytes::from_static(b"{}"));
    };
    // Skipping a string does not check its UTF-8, as reading it into a value did: that is checked first, whole.
    if std::str::from_utf8(&bytes).is_err()
        || serde_json::from_slice::<serde::de::IgnoredAny>(&bytes).is_err()
    {
        return Err(parse_failure(&bytes));
    }
    Ok(bytes)
}

pub(crate) fn client_context_of(r: &Req) -> Option<String> {
    let raw = r.opt("ClientContext")?;
    let bytes = base64::decode(raw)?;
    String::from_utf8(bytes).ok()
}

/// The answer of a synchronous invoke. The payload is not in it: the reply carries those bytes whole beside it (`answered`).
fn sync_reply(out: &RunOut, executed: &str, tail: bool) -> Value {
    let mut reply = Map::new();
    reply.insert("StatusCode".into(), json!(200));
    reply.insert("ExecutedVersion".into(), json!(executed));
    if out.error {
        reply.insert("FunctionError".into(), json!("Unhandled"));
    }
    if tail {
        let bytes = out.log.as_bytes();
        let from = bytes.len().saturating_sub(4096);
        let cut = bytes.get(from..).unwrap_or(bytes);
        reply.insert("LogResult".into(), json!(base64::encode(cut)));
    }
    Value::Object(reply)
}

/// The answer of an asynchronous invoke: `202`, whether the event was queued or dropped (AWS accepts it either way). A drop is
/// marked for the console's Live record (`summary::DROPPED`, never sent on the wire).
pub(crate) fn accepted(kept: bool) -> Value {
    if kept {
        json!({"StatusCode": 202})
    } else {
        json!({"StatusCode": 202, (aws_peers::summary::DROPPED): true})
    }
}

/// The reply of a synchronous invoke: its JSON, and the function's answer held whole beside it, never written as base64 and read back.
pub(crate) fn answered(out: RunOut, executed: &str, tail: bool) -> RestReply {
    RestReply {
        json: sync_reply(&out, executed, tail),
        body: Some(aws_wire::rest::whole(out.payload)),
        ..RestReply::default()
    }
}

pub(crate) fn trace_of(call: &Call) -> String {
    let id = call.id.as_str().replace('-', "");
    let root: String = id.chars().take(24).collect();
    format!(
        "Root=1-{}-{root};Sampled=0",
        id.chars().take(8).collect::<String>()
    )
}

pub(crate) fn join(a: &Bytes, b: &Bytes) -> Bytes {
    if a.is_empty() {
        return b.clone();
    }
    let mut out = Vec::with_capacity(a.len() + b.len());
    out.extend_from_slice(a);
    out.extend_from_slice(b);
    Bytes::from(out)
}

/// A process that could not be started is a function error, as a broken init is on AWS: a 200 with the reason in the payload.
pub(crate) fn launch_failure(why: &str) -> RunOut {
    RunOut {
        payload: Bytes::from(
            json!({"errorMessage": why, "errorType": "Runtime.LaunchError"}).to_string(),
        ),
        error: true,
        request_id: aws_seal::uuid_v4().unwrap_or_default(),
        content_type: None,
        log: String::new(),
    }
}

pub(crate) fn init_document(failure: &InitFailure, id: &str, timeout: Duration) -> Bytes {
    match failure {
        InitFailure::Timeout => Bytes::from(json!({"errorMessage": format!("RequestId: {id} Error: Task timed out during init after 10.00 seconds"), "errorType": "Runtime.InitTimeout"}).to_string()),
        InitFailure::Exited(Some(reported)) => reported.clone(),
        InitFailure::Exited(None) => {
            let _ = timeout;
            exit_document(id, None)
        }
    }
}

pub(crate) fn exit_document(id: &str, status: Option<&str>) -> Bytes {
    let how = status.map_or_else(
        || "exit status 1".to_owned(),
        |s| s.replace("exit status: ", "exit status "),
    );
    Bytes::from(json!({"errorMessage": format!("RequestId: {id} Error: Runtime exited with error: {how}"), "errorType": "Runtime.ExitError"}).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(payload: Option<&[u8]>) -> Req {
        Req {
            body: Value::Null,
            caller: aws_wire::service::Caller {
                access_key_id: "test".into(),
                account_id: "000000000000".into(),
                region: "us-east-1".into(),
                endpoint: String::new(),
                service: None,
                session_token: None,
            },
            payload: payload.map(Bytes::copy_from_slice),
        }
    }

    #[test]
    fn the_payload_is_the_trees_bytes_checked_as_json() {
        assert_eq!(payload_of(&req(None)).unwrap(), Bytes::from_static(b"{}"));
        assert_eq!(
            payload_of(&req(Some(b""))).unwrap(),
            Bytes::from_static(b"{}")
        );
        let json = br#"{"a":[1,"\u00e9",{"b":null}]}"#;
        assert_eq!(
            payload_of(&req(Some(json))).unwrap(),
            Bytes::from_static(json)
        );
        assert!(payload_of(&req(Some(b"{not json"))).is_err());
        assert!(payload_of(&req(Some(b"{} trailing"))).is_err());
        // Text that is not UTF-8 is not JSON, even inside a string a check that skips strings would pass.
        assert!(payload_of(&req(Some(b"\"\xff\""))).is_err());
    }

    #[test]
    fn a_payload_that_is_not_json_is_refused_in_jacksons_words() {
        let e = parse_failure(b"{not json");
        assert_eq!(e.code, "InvalidRequestContentException");
        assert!(
            e.message.starts_with("Could not parse request body into json: Could not parse payload into json: Unexpected character ('n' (code 110)): was expecting double-quote to start field name\n at [Source: REDACTED (`StreamReadFeature.INCLUDE_SOURCE_IN_LOCATION` disabled); line: 1, column: 2]"),
            "{}",
            e.message
        );
    }

    /// Before: an event dropped by a full queue answered as any other `202`, so the console's Live record showed nothing. The drop is
    /// read as the call's summary, and the answer on the wire stays AWS's.
    #[test]
    fn a_dropped_event_is_marked_for_the_record_and_not_on_the_wire() {
        use aws_peers::summary::{Kind, Summary, from_output};
        let invoke = crate::TABLES.find_op("Invoke").unwrap_or_default();
        let read = |v: &Value| {
            let mut s = Summary::NONE;
            from_output("Lambda", "Invoke", v, &mut s);
            s.kind
        };
        assert_eq!(read(&accepted(false)), Kind::Dropped);
        assert_eq!(read(&accepted(true)), Kind::None);
        let wire = aws_wire::rest::jsonout::render(&crate::TABLES, invoke, &accepted(false), None);
        assert_eq!((wire.status, wire.body.len()), (202, 0));
        assert!(
            wire.headers.iter().all(|(k, _)| !k.contains("doze")),
            "{:?}",
            wire.headers
        );
    }
}
