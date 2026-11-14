//! Lambda: a bare function ARN (the input is the payload, the result the function's answer) and `lambda:invoke` (the parameters are
//! the `Invoke` request, the result is `{ExecutedVersion, Payload, StatusCode, ...}`). A function that fails is the failure named
//! by its `errorType` (`Lambda.Unknown` when it gives none) with its error document as the cause; an API error is `Lambda.<Code>`.

use std::sync::Arc;

use aws_asl::Failure;
use aws_peers::Peers;
use aws_wire::service::Caller;
use serde_json::{Value, json};

use super::{failure, with_metadata};

/// What one `Invoke` answered.
struct Answer {
    status: u16,
    version: Option<String>,
    function_error: Option<String>,
    payload: String,
}

async fn invoke(
    peers: &Arc<Peers>,
    caller: &Caller,
    resource: &str,
    body: &Value,
) -> Result<Answer, Failure> {
    let mut reply = peers
        .call_rest_as(caller, "lambda", "Invoke", body, None)
        .await
        .map_err(|e| failure("Lambda", resource, &e))?;
    let mut payload = Vec::new();
    if let Some(mut stream) = reply.body.take() {
        while let Some(chunk) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            let chunk =
                chunk.map_err(|e| Failure::new("Lambda.ServiceException", e.to_string()))?;
            payload.extend_from_slice(&chunk);
        }
    }
    let text = |k: &str| reply.json.get(k).and_then(Value::as_str).map(str::to_owned);
    Ok(Answer {
        status: reply
            .json
            .get("StatusCode")
            .and_then(Value::as_u64)
            .and_then(|s| u16::try_from(s).ok())
            .unwrap_or(200),
        version: text("ExecutedVersion"),
        function_error: text("FunctionError"),
        payload: String::from_utf8_lossy(&payload).into_owned(),
    })
}

/// The failure of a function that threw.
fn thrown(payload: &str) -> Failure {
    let doc: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
    let error = doc
        .get("errorType")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("Lambda.Unknown");
    Failure::new(error, payload)
}

pub(super) async fn bare(
    peers: &Arc<Peers>,
    caller: &Caller,
    resource: &str,
    input: &Value,
) -> Result<Value, Failure> {
    let body = json!({
        "FunctionName": resource,
        "InvocationType": "RequestResponse",
        "Payload": aws_core::base64::encode(input.to_string().as_bytes()),
    });
    let a = invoke(peers, caller, resource, &body).await?;
    if a.function_error.is_some() {
        return Err(thrown(&a.payload));
    }
    Ok(serde_json::from_str(&a.payload).unwrap_or(Value::Null))
}

pub(super) async fn optimized(
    peers: &Arc<Peers>,
    caller: &Caller,
    input: &Value,
) -> Result<Value, Failure> {
    let resource = "arn:aws:states:::lambda:invoke";
    let mut body = serde_json::Map::new();
    for k in [
        "FunctionName",
        "InvocationType",
        "Qualifier",
        "ClientContext",
        "LogType",
    ] {
        if let Some(v) = input.get(k) {
            body.insert(k.to_owned(), v.clone());
        }
    }
    let payload = input
        .get("Payload")
        .map_or_else(|| "{}".to_owned(), Value::to_string);
    body.insert(
        "Payload".into(),
        json!(aws_core::base64::encode(payload.as_bytes())),
    );
    let a = invoke(peers, caller, resource, &Value::Object(body)).await?;
    if a.function_error.is_some() {
        return Err(thrown(&a.payload));
    }
    let parsed = if a.payload.is_empty() {
        json!("")
    } else {
        serde_json::from_str(&a.payload).unwrap_or(Value::String(a.payload.clone()))
    };
    let mut out = json!({"Payload": parsed, "StatusCode": a.status});
    if let (Some(v), Some(m)) = (a.version, out.as_object_mut()) {
        m.insert("ExecutedVersion".into(), json!(v));
    }
    Ok(with_metadata(out, a.status))
}
