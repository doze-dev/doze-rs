//! Universal targets: `arn:aws:scheduler:::aws-sdk:<service>:<action>` with `Input` as the request, for the services doze-aws runs.
//! The action is the operation (`sendMessage` is `SendMessage`). Step Functions' members are lowercase-initial on its wire and
//! PascalCase in an SDK request, so its keys are turned on the way in. Lambda `Invoke` sends `Payload` (text or JSON) as the bytes of
//! its text. S3 operations with a body (`PutObject`) send `Body` as the object.

use std::sync::Arc;

use aws_peers::Peers;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Caller};
use serde_json::{Map, Value};

use crate::deliver::{Failure, Outcome};
use crate::targets::bus_name;

pub(crate) async fn call(
    peers: &Arc<Peers>,
    caller: &Caller,
    (sdk, action): (&str, &str),
    input: Option<&str>,
) -> Outcome {
    let Some(target) = bus_name(sdk) else {
        return Outcome::NotLocal(format!("the service {sdk} does not run in doze-aws"));
    };
    let op = upper_first(action);
    if peers
        .tables_of(target)
        .is_none_or(|t| t.find_op(&op).is_none())
    {
        return Outcome::NotLocal(format!("{sdk} has no operation {op} here, or does not run"));
    }
    let request: Value = match input.map(serde_json::from_str).transpose() {
        Ok(v) => v.unwrap_or_else(|| Value::Object(Map::new())),
        // (memory) AWS does not check the input when the schedule is made; the invocation fails.
        Err(e) => {
            return Failure::fatal(
                "ValidationException",
                format!("The Input is not valid JSON: {e}"),
            );
        }
    };
    let mut body = request.as_object().cloned().unwrap_or_default();
    let got = match target {
        "s3" => s3(peers, caller, &op, body).await,
        "states" => peers
            .call_as(caller, target, &op, &recase(&Value::Object(body)))
            .await
            .map(drop),
        _ => {
            if target == "lambda"
                && op == "Invoke"
                && let Some(p) = body.get("Payload").map(as_text)
            {
                body.insert(
                    "Payload".into(),
                    Value::String(aws_core::base64::encode(p.as_bytes())),
                );
            }
            peers
                .call_as(caller, target, &op, &Value::Object(body))
                .await
                .map(drop)
        }
    };
    match got {
        Ok(()) => Outcome::Sent,
        Err(e) => Failure::of(&e),
    }
}

async fn s3(
    peers: &Arc<Peers>,
    caller: &Caller,
    op: &str,
    mut body: Map<String, Value>,
) -> Result<(), ApiError> {
    let upload: Option<Box<dyn aws_wire::rest::Upload>> = body.remove("Body").map(|b| {
        let bytes = as_text(&b).into_bytes();
        let len = bytes.len() as u64;
        Box::new(Whole(Some(bytes), len)) as Box<dyn aws_wire::rest::Upload>
    });
    peers
        .call_rest_as(caller, "s3", op, &Value::Object(body), upload)
        .await
        .map(drop)
}

/// A request body sent in one piece.
struct Whole(Option<Vec<u8>>, u64);

impl aws_wire::rest::Upload for Whole {
    fn next(&mut self) -> BoxFuture<'_, Result<Option<bytes::Bytes>, ApiError>> {
        let next = self.0.take().map(bytes::Bytes::from);
        Box::pin(async move { Ok(next) })
    }

    fn trailers(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    fn length(&self) -> Option<u64> {
        Some(self.1)
    }
}

fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub(crate) fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// Every object key with its first letter lower-cased (Step Functions' wire).
fn recase(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, v)| {
                    let mut c = k.chars();
                    let key = c
                        .next()
                        .map_or_else(String::new, |f| f.to_lowercase().chain(c).collect());
                    (key, recase(v))
                })
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(recase).collect()),
        other => other.clone(),
    }
}
