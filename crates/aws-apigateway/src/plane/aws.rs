//! AWS service integrations, called over the peer bus as API Gateway: a REST API's `AWS` integration
//! (`arn:aws:apigateway:<region>:<service>:action/<Operation>` with the mapped body as the request, JSON or form-encoded; or SQS's
//! `path/<account>/<queue>`), and an HTTP API's first-class integrations (`integrationSubtype`: `SQS-SendMessage`,
//! `EventBridge-PutEvents`, `Kinesis-PutRecord`, `StepFunctions-StartExecution` and the rest) with their parameter mappings. A
//! service that is not running here answers what the bus says, never a pretend success.

use aws_wire::service::{Caller, WebReply};
use bytes::Bytes;
use serde_json::{Map, Value, json};

use super::custom::{Backend, Mapped};
use super::mapping::{query_pairs, v2_source};
use super::{Found, Incoming, Route, reply};
use crate::Inner;
use crate::doc::text;

fn caller(found: &Found, region: &str) -> Caller {
    Caller {
        access_key_id: "apigateway".into(),
        account_id: found.account.clone(),
        region: region.to_owned(),
        endpoint: "http://localhost".into(),
        service: None,
        session_token: None,
    }
}

/// Calls `operation` of `service` as API Gateway: the answer, or the service's refusal as a status and a JSON body.
async fn call(
    inner: &Inner,
    found: &Found,
    region: &str,
    service: &str,
    operation: &str,
    body: &Value,
) -> (u16, Value) {
    let Some(peers) = inner.peers.as_ref() else {
        return (500, json!({"message": "no peer bus"}));
    };
    match peers
        .call(
            "apigateway",
            &caller(found, region),
            service,
            operation,
            body,
        )
        .await
    {
        Ok(v) => (200, v),
        Err(e) => (e.status, json!({"__type": e.code, "message": e.message})),
    }
}

/// A form-encoded body (`Action=SendMessage&MessageBody=hi`) as the JSON members of the operation; `Action` and `Version` are not
/// members.
fn form(body: &str) -> Map<String, Value> {
    query_pairs(body)
        .into_iter()
        .filter(|(k, _)| k != "Action" && k != "Version")
        .map(|(k, v)| (k, Value::String(v)))
        .collect()
}

/// The operation and request an `action/` or `path/` URI and the mapped body make.
fn request_of(
    uri: &str,
    service: &str,
    rest: &str,
    mapped: &str,
) -> Result<(String, Map<String, Value>), String> {
    let parsed: Option<Map<String, Value>> = serde_json::from_str::<Value>(mapped)
        .ok()
        .and_then(|v| v.as_object().cloned());
    if let Some(action) = rest.strip_prefix("action/") {
        let body = parsed.unwrap_or_else(|| form(mapped));
        return Ok((action.split('&').next().unwrap_or(action).to_owned(), body));
    }
    let path = rest
        .strip_prefix("path/")
        .ok_or_else(|| format!("{uri} names neither an action nor a path"))?;
    let action = query_pairs(mapped)
        .into_iter()
        .find(|(k, _)| k == "Action")
        .map(|(_, v)| v)
        .or_else(|| path.split_once("?Action=").map(|(_, a)| a.to_owned()))
        .ok_or("an AWS path integration needs an Action")?;
    if service != "sqs" {
        return Err(format!(
            "path integrations to {service} are not built in doze-aws (action integrations are)"
        ));
    }
    let queue = path.split('?').next().unwrap_or(path);
    let mut b = parsed.unwrap_or_else(|| form(mapped));
    b.insert(
        "QueueUrl".into(),
        json!(format!("http://localhost/{queue}")),
    );
    Ok((action, b))
}

/// A REST API's `AWS` integration.
pub(crate) async fn rest(
    inner: &Inner,
    found: &Found,
    route: &Route,
    _req: &Incoming,
    mapped: &Mapped,
) -> Result<Backend, String> {
    let uri = super::lambda::fill_vars(
        text(&route.integration, "uri").unwrap_or_default(),
        found.variables(),
    );
    let mut parts = uri.splitn(6, ':');
    let (region, service, rest) = match (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) {
        (Some("arn"), Some(_), Some("apigateway"), Some(region), Some(service), Some(rest)) => {
            (region, service, rest)
        }
        _ => return Err(format!("{uri} is not an AWS integration URI")),
    };
    let service = service.rsplit('.').next().unwrap_or(service);
    let (operation, mut body) = request_of(&uri, service, rest, &mapped.body)?;
    if service == "sqs"
        && let Some(url) = body
            .get("QueueUrl")
            .and_then(Value::as_str)
            .map(str::to_owned)
        && !url.starts_with("http")
    {
        body.insert("QueueUrl".into(), json!(format!("http://localhost/{url}")));
    }
    let (status, answer) = call(
        inner,
        found,
        region,
        service,
        &operation,
        &Value::Object(body),
    )
    .await;
    Ok(Backend {
        selector: status.to_string(),
        headers: vec![("Content-Type".into(), "application/json".into())],
        body: answer.to_string(),
    })
}

/// The service and operation an HTTP API's `integrationSubtype` calls, and how its parameters become the request.
fn subtype(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name {
        "SQS-SendMessage" => ("sqs", "SendMessage"),
        "SQS-ReceiveMessage" => ("sqs", "ReceiveMessage"),
        "SQS-DeleteMessage" => ("sqs", "DeleteMessage"),
        "SQS-PurgeQueue" => ("sqs", "PurgeQueue"),
        "EventBridge-PutEvents" => ("events", "PutEvents"),
        "Kinesis-PutRecord" => ("kinesis", "PutRecord"),
        "StepFunctions-StartExecution" => ("states", "StartExecution"),
        "StepFunctions-StartSyncExecution" => ("states", "StartSyncExecution"),
        "StepFunctions-StopExecution" => ("states", "StopExecution"),
        _ => return None,
    })
}

/// An HTTP API's AWS service integration.
pub(crate) async fn v2(
    inner: &Inner,
    found: &Found,
    route: &Route,
    req: &Incoming,
    ctx: &Value,
) -> WebReply {
    let name = text(&route.integration, "integrationSubtype").unwrap_or_default();
    let Some((service, operation)) = subtype(name) else {
        return reply::message(
            500,
            &format!("The integration subtype {name} is not built in doze-aws"),
        );
    };
    let vars = found
        .variables()
        .map_or(Value::Null, |m| Value::Object(m.clone()));
    let raw = super::event::raw_path(found);
    let mut params: Map<String, Value> = crate::doc::obj(&route.integration, "requestParameters")
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| {
            Some((
                k.clone(),
                Value::String(v2_source(v.as_str()?, req, &route.params, &raw, ctx, &vars)),
            ))
        })
        .collect();
    let region = params
        .remove("Region")
        .and_then(|r| r.as_str().map(str::to_owned))
        .unwrap_or_else(|| found.region.clone());
    let body = shape(operation, params);
    let (status, answer) = call(inner, found, &region, service, operation, &body).await;
    reply::of(
        status,
        vec![("Content-Type".into(), "application/json".into())],
        Bytes::from(answer.to_string()),
    )
}

/// The request of a subtype's operation from its string parameters (PutEvents takes one entry; Kinesis data is base64 on the wire).
fn shape(operation: &str, mut p: Map<String, Value>) -> Value {
    match operation {
        "PutEvents" => json!({"Entries": [Value::Object(p)]}),
        "PutRecord" => {
            if let Some(Value::String(d)) = p.get("Data").cloned() {
                p.insert("Data".into(), json!(aws_core::base64::encode(d.as_bytes())));
            }
            Value::Object(p)
        }
        "SendMessage" | "ReceiveMessage" | "DeleteMessage" | "PurgeQueue" => {
            if let Some(Value::String(u)) = p.get("QueueUrl").cloned()
                && !u.starts_with("http")
            {
                p.insert("QueueUrl".into(), json!(format!("http://localhost/{u}")));
            }
            for k in [
                "DelaySeconds",
                "MaxNumberOfMessages",
                "VisibilityTimeout",
                "WaitTimeSeconds",
            ] {
                if let Some(n) = p
                    .get(k)
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse::<i64>().ok())
                {
                    p.insert(k.into(), json!(n));
                }
            }
            Value::Object(p)
        }
        _ => Value::Object(p),
    }
}
