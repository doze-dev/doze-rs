//! Lambda's `Invoke` from the console: synchronous (`RequestResponse`) always, the payload sent and answered as text (the
//! model carries it as base64), and the log tail (`LogType: Tail`, the last 4 KB of the run's log) decoded when asked for.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::model::Invoked;
use crate::resources::bus::Bus;

/// The request the page sends: the model's, but with `Payload` as text and `LogType` the only other choice.
///
/// # Errors
/// `BadRequest` when it names no function or its payload is not text.
pub fn request(asked: &Value) -> Result<Value, ApiError> {
    let text = |k: &str| asked.get(k).and_then(Value::as_str);
    let function = text("FunctionName")
        .filter(|f| !f.is_empty())
        .ok_or_else(|| ApiError::new(400, "BAD_REQUEST", "Invoke names a FunctionName."))?;
    let payload = match asked.get("Payload") {
        None | Some(Value::Null) => "",
        Some(Value::String(s)) => s.as_str(),
        Some(_) => {
            return Err(ApiError::new(
                400,
                "BAD_REQUEST",
                "The payload is sent as text (the JSON event, written out).",
            ));
        }
    };
    let mut body = Map::new();
    body.insert("FunctionName".into(), json!(function));
    body.insert("InvocationType".into(), json!("RequestResponse"));
    body.insert(
        "LogType".into(),
        json!(if text("LogType") == Some("Tail") {
            "Tail"
        } else {
            "None"
        }),
    );
    if let Some(q) = text("Qualifier").filter(|q| !q.is_empty()) {
        body.insert("Qualifier".into(), json!(q));
    }
    if !payload.is_empty() {
        body.insert(
            "Payload".into(),
            json!(aws_core::base64::encode(payload.as_bytes())),
        );
    }
    Ok(Value::Object(body))
}

/// What `Invoke` answered, as the page shows it.
#[must_use]
pub fn answer(reply: &Value) -> Invoked {
    let text = |k: &str| reply.get(k).and_then(Value::as_str).map(str::to_owned);
    let decoded = |k: &str| {
        text(k)
            .and_then(|b| aws_core::base64::decode(&b))
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    };
    Invoked {
        status: reply
            .get("StatusCode")
            .and_then(Value::as_u64)
            .and_then(|s| u16::try_from(s).ok())
            .unwrap_or(200),
        function_error: text("FunctionError"),
        executed_version: text("ExecutedVersion"),
        payload: decoded("Payload").unwrap_or_default(),
        log: decoded("LogResult"),
    }
}

/// Invokes the function `asked` names, as the console.
///
/// # Errors
/// A request that is not one, or Lambda's refusal (no such function, a throttle, a runtime that cannot start).
pub async fn invoke(bus: &Bus, asked: &Value) -> Result<Invoked, ApiError> {
    let body = request(asked)?;
    let reply = bus.call("lambda", "Invoke", &body).await?;
    Ok(answer(&reply))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{answer, request};

    #[test]
    fn the_payload_goes_as_base64_and_the_call_is_synchronous() {
        let body = request(&json!({"FunctionName": "f", "Payload": "{\"a\":1}", "LogType": "Tail", "InvocationType": "Event"}))
            .unwrap_or_default();
        assert_eq!(body["InvocationType"], "RequestResponse");
        assert_eq!(body["LogType"], "Tail");
        assert_eq!(body["Payload"], aws_core::base64::encode(b"{\"a\":1}"));
        let bare = request(&json!({"FunctionName": "f"})).unwrap_or_default();
        assert_eq!(bare["LogType"], "None");
        assert!(bare.get("Payload").is_none());
        assert!(request(&json!({"Payload": "{}"})).is_err());
        assert!(request(&json!({"FunctionName": "f", "Payload": {"a": 1}})).is_err());
    }

    #[test]
    fn the_answer_is_read_back_as_text() {
        let got = answer(&json!({
            "StatusCode": 200,
            "FunctionError": "Unhandled",
            "ExecutedVersion": "$LATEST",
            "Payload": aws_core::base64::encode(b"{\"errorMessage\":\"boom\"}"),
            "LogResult": aws_core::base64::encode(b"START RequestId: 1\nEND RequestId: 1\n"),
        }));
        assert_eq!(got.function_error.as_deref(), Some("Unhandled"));
        assert_eq!(got.payload, "{\"errorMessage\":\"boom\"}");
        assert!(got.log.unwrap_or_default().starts_with("START"));
        assert_eq!(answer(&json!({})).log, None);
    }
}
