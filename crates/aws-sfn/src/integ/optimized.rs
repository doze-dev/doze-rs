//! The optimized integrations of SQS (`sendMessage`), SNS (`publish`), EventBridge (`putEvents`) and DynamoDB (`getItem`,
//! `putItem`, `updateItem`, `deleteItem`): the parameters are the service's request (a message or detail given as JSON is sent as
//! its text), the result its answer with AWS's SDK metadata beside it.

use std::sync::Arc;

use aws_asl::Failure;
use aws_peers::Peers;
use aws_wire::service::Caller;
use serde_json::{Map, Value};

use super::{as_text, failure, with_metadata};

fn object(input: &Value) -> Map<String, Value> {
    input.as_object().cloned().unwrap_or_default()
}

pub(super) async fn sqs(
    peers: &Arc<Peers>,
    caller: &Caller,
    input: &Value,
) -> Result<Value, Failure> {
    let mut body = object(input);
    if let Some(m) = body.get("MessageBody").map(as_text) {
        body.insert("MessageBody".into(), Value::String(m));
    }
    let got = peers
        .call_as(caller, "sqs", "SendMessage", &Value::Object(body))
        .await
        .map_err(|e| failure("SQS", "arn:aws:states:::sqs:sendMessage", &e))?;
    Ok(with_metadata(got, 200))
}

pub(super) async fn sns(
    peers: &Arc<Peers>,
    caller: &Caller,
    input: &Value,
) -> Result<Value, Failure> {
    let mut body = object(input);
    if let Some(m) = body.get("Message").map(as_text) {
        body.insert("Message".into(), Value::String(m));
    }
    let got = peers
        .call_as(caller, "sns", "Publish", &Value::Object(body))
        .await
        .map_err(|e| failure("SNS", "arn:aws:states:::sns:publish", &e))?;
    Ok(with_metadata(got, 200))
}

pub(super) async fn events(
    peers: &Arc<Peers>,
    caller: &Caller,
    input: &Value,
) -> Result<Value, Failure> {
    let mut body = object(input);
    if let Some(Value::Array(entries)) = body.get_mut("Entries") {
        for e in entries.iter_mut() {
            if let Some(m) = e.as_object_mut()
                && let Some(d) = m.get("Detail").map(as_text)
            {
                m.insert("Detail".into(), Value::String(d));
            }
        }
    }
    let resource = "arn:aws:states:::events:putEvents";
    let got = peers
        .call_as(caller, "events", "PutEvents", &Value::Object(body))
        .await
        .map_err(|e| failure("EventBridge", resource, &e))?;
    if got
        .get("FailedEntryCount")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        > 0
    {
        // (memory)
        return Err(Failure::new("EventBridge.FailedEntry", got.to_string()));
    }
    Ok(with_metadata(got, 200))
}

pub(super) async fn dynamodb(
    peers: &Arc<Peers>,
    caller: &Caller,
    action: &str,
    input: &Value,
) -> Result<Value, Failure> {
    let op = upper_first(action);
    let resource = format!("arn:aws:states:::dynamodb:{action}");
    let got = peers
        .call_as(caller, "dynamodb", &op, input)
        .await
        .map_err(|e| failure("DynamoDB", &resource, &e))?;
    Ok(with_metadata(got, 200))
}

/// `getItem` to `GetItem`.
pub(super) fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}
