//! `arn:aws:states:::aws-sdk:<service>:<action>` for every service that runs here: the action is the operation (`getObject` is
//! `GetObject`), the parameters its request, the result its answer, and an error `<Service>.<Code>Exception` with the service named
//! as AWS's SDK names it (`S3`, `DynamoDb`, `Sfn`, ...; memory). Step Functions' own members are lowercase-initial on the wire and
//! PascalCase in a task, so its keys are turned on the way in and out. S3's `GetObject` body comes back as text in `Body`, and a
//! `PutObject` sends `Body` as the object.

use std::sync::Arc;

use aws_asl::Failure;
use aws_peers::Peers;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Caller};
use bytes::Bytes;
use serde_json::{Map, Value};

use super::optimized::upper_first;
use super::{failure, not_local_failure};

/// An SDK service id: the service's name on the bus, and the name its errors are prefixed with.
fn service(id: &str) -> Option<(&'static str, &'static str)> {
    Some(match id {
        "s3" => ("s3", "S3"),
        "dynamodb" => ("dynamodb", "DynamoDb"),
        "sqs" => ("sqs", "Sqs"),
        "sns" => ("sns", "Sns"),
        "lambda" => ("lambda", "Lambda"),
        "sfn" => ("states", "Sfn"),
        "eventbridge" => ("events", "EventBridge"),
        "secretsmanager" => ("secretsmanager", "SecretsManager"),
        "ssm" => ("ssm", "Ssm"),
        "kms" => ("kms", "Kms"),
        "kinesis" => ("kinesis", "Kinesis"),
        "iam" => ("iam", "Iam"),
        "sts" => ("sts", "Sts"),
        "cloudwatch" => ("monitoring", "CloudWatch"),
        "cloudwatchlogs" => ("logs", "CloudWatchLogs"),
        _ => return None,
    })
}

/// A request body sent in one piece.
struct Whole(Option<Bytes>, u64);

impl aws_wire::rest::Upload for Whole {
    fn next(&mut self) -> BoxFuture<'_, Result<Option<Bytes>, ApiError>> {
        let next = self.0.take();
        Box::pin(async move { Ok(next) })
    }

    fn trailers(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    fn length(&self) -> Option<u64> {
        Some(self.1)
    }
}

pub(super) async fn call(
    peers: &Arc<Peers>,
    caller: &Caller,
    resource: &str,
    sdk: &str,
    input: &Value,
) -> Result<Value, Failure> {
    let (id, action) = sdk.split_once(':').unwrap_or((sdk, ""));
    let Some((target, prefix)) = service(id) else {
        return Err(not_local_failure(
            resource,
            &format!("the service {id} does not run in doze-aws"),
        ));
    };
    let op = upper_first(action);
    if peers
        .tables_of(target)
        .is_none_or(|t| t.find_op(&op).is_none())
    {
        return Err(not_local_failure(
            resource,
            &format!("{id} has no operation {op} here, or does not run"),
        ));
    }
    if matches!(target, "s3" | "lambda") {
        return rest(peers, caller, resource, target, prefix, &op, input).await;
    }
    let body = if target == "states" {
        recase(input, false)
    } else {
        input.clone()
    };
    let got = peers
        .call_as(caller, target, &op, &body)
        .await
        .map_err(|e| failure(prefix, resource, &e))?;
    Ok(if target == "states" {
        recase(&got, true)
    } else {
        got
    })
}

#[allow(clippy::too_many_arguments)]
async fn rest(
    peers: &Arc<Peers>,
    caller: &Caller,
    resource: &str,
    target: &str,
    prefix: &str,
    op: &str,
    input: &Value,
) -> Result<Value, Failure> {
    let mut body = input.as_object().cloned().unwrap_or_default();
    let mut upload: Option<Box<dyn aws_wire::rest::Upload>> = None;
    if op == "PutObject"
        && let Some(b) = body.remove("Body")
    {
        let bytes = Bytes::from(super::as_text(&b).into_bytes());
        let len = bytes.len() as u64;
        upload = Some(Box::new(Whole(Some(bytes), len)));
    }
    if op == "Invoke"
        && let Some(p) = body.get("Payload").map(super::as_text)
    {
        body.insert(
            "Payload".into(),
            Value::String(aws_core::base64::encode(p.as_bytes())),
        );
    }
    let mut reply = peers
        .call_rest_as(caller, target, op, &Value::Object(body), upload)
        .await
        .map_err(|e| failure(prefix, resource, &e))?;
    let mut out = reply.json.clone();
    if let Some(mut stream) = reply.body.take() {
        let mut bytes = Vec::new();
        while let Some(chunk) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            let chunk = chunk
                .map_err(|e| Failure::new(format!("{prefix}.SdkClientException"), e.to_string()))?;
            bytes.extend_from_slice(&chunk);
        }
        let key = if op == "Invoke" { "Payload" } else { "Body" };
        if let Some(m) = out.as_object_mut() {
            m.insert(
                key.into(),
                Value::String(String::from_utf8_lossy(&bytes).into_owned()),
            );
        }
    }
    Ok(out)
}

/// Every object key with its first letter upper-cased (`up`) or lower-cased.
fn recase(v: &Value, up: bool) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, v)| {
                    let mut c = k.chars();
                    let key = c.next().map_or_else(String::new, |f| {
                        if up {
                            f.to_uppercase().chain(c).collect()
                        } else {
                            f.to_lowercase().chain(c).collect()
                        }
                    });
                    (key, recase(v, up))
                })
                .collect::<Map<_, _>>(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(|x| recase(x, up)).collect()),
        other => other.clone(),
    }
}
