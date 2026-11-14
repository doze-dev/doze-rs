//! Custom resources (`AWS::CloudFormation::CustomResource` and `Custom::*`), native first (decision D6): the CDK's own helpers
//! (`Custom::S3AutoDeleteObjects`, `Custom::S3BucketNotifications`, `Custom::LogRetention`, `Custom::CDKBucketDeployment*`,
//! `Custom::AWS`) are done here, in-process, because their real handlers are Node functions that import the AWS SDK and expect the
//! Lambda runtime. Any other custom resource is run as AWS runs it: its `ServiceToken` function (or SNS topic) is sent the request
//! object with a presigned response URL, and the resource waits for the answer PUT there.

mod aws_call;
mod cdk;
mod deploy;
mod unzip;

use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::j::J;
use crate::model::Res;
use crate::res::{Done, Made, Rc, text};

/// What a custom resource is asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    Create,
    Update,
    Delete,
}

impl Request {
    fn name(self) -> &'static str {
        match self {
            Self::Create => "Create",
            Self::Update => "Update",
            Self::Delete => "Delete",
        }
    }
}

/// Whether the archive reader accepts `bytes` (for the fuzz target).
pub(crate) fn check_zip(bytes: &[u8]) -> bool {
    unzip::files(bytes).is_ok()
}

/// Whether `ty` is a custom resource.
pub(crate) fn is_custom(ty: &str) -> bool {
    ty.starts_with("Custom::") || ty == "AWS::CloudFormation::CustomResource"
}

pub(crate) async fn create(rc: &Rc, p: &Value) -> Done {
    run(rc, Request::Create, None, p).await
}

pub(crate) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    run(rc, Request::Update, Some(old), p).await
}

pub(crate) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    run(rc, Request::Delete, Some(res), &res.props_value())
        .await
        .map(|_| ())
}

async fn run(rc: &Rc, req: Request, old: Option<&Res>, p: &Value) -> Done {
    if let Some(done) = cdk::native(rc, req, old, p).await {
        return done;
    }
    remote(rc, req, old, p).await
}

/// The request object AWS sends a custom resource's handler.
fn request_object(
    rc: &Rc,
    req: Request,
    old: Option<&Res>,
    p: &Value,
    url: &str,
    request_id: &str,
) -> Value {
    let mut props = p.clone();
    if let Some(o) = props.as_object_mut() {
        o.insert("ServiceToken".into(), p.at("ServiceToken").clone());
    }
    let mut e = json!({"RequestType": req.name(), "ServiceToken": p.at("ServiceToken"), "ResponseURL": url,
        "StackId": rc.stack_id, "RequestId": request_id, "LogicalResourceId": rc.logical, "ResourceType": rc.ty,
        "ResourceProperties": props});
    if let Some(old) = old {
        e.put(
            "PhysicalResourceId",
            old.physical.clone().unwrap_or_default().into(),
        );
        if req == Request::Update {
            e.put("OldResourceProperties", old.props_value());
        }
    }
    e
}

/// A handler run as AWS runs it, by its `ServiceToken`.
async fn remote(rc: &Rc, req: Request, old: Option<&Res>, p: &Value) -> Done {
    let token = text(p, "ServiceToken").unwrap_or_default();
    if token.is_empty() {
        // (memory)
        return Err(rc.handler("ServiceToken is required", "InvalidRequest"));
    }
    let request_id = aws_seal::uuid_v4().unwrap_or_default();
    let responses = &rc.runner.responses;
    let (url, rx) = responses.expect(
        &rc.caller.endpoint,
        &rc.caller.region,
        &rc.stack_id,
        &rc.logical,
    );
    let event = request_object(rc, req, old, p, &url, &request_id);
    let started = if token.contains(":sns:") {
        let body = json!({"TopicArn": token, "Message": event.to_string()});
        rc.call("sns", "Publish", body).await.map(|_| None)
    } else if token.contains(":lambda:") {
        invoke(rc, &token, &event).await
    } else {
        // (memory)
        return Err(format!(
            "Invalid service token {token}: a custom resource's ServiceToken is a Lambda function or an SNS topic"
        ));
    };
    let failed_early = match started {
        Ok(f) => f,
        Err(e) => {
            responses.forget(&url);
            return Err(rc.fail(
                if token.contains(":sns:") {
                    "sns"
                } else {
                    "lambda"
                },
                &e,
            ));
        }
    };
    let wait = text(p, "ServiceTimeout")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(3600);
    let answer = match (
        failed_early,
        tokio::time::timeout(Duration::from_secs(wait), rx).await,
    ) {
        (_, Ok(Ok(v))) => v,
        (Some(error), _) => {
            responses.forget(&url);
            // The function ended in an error and never answered: say so now rather than waiting out the hour.
            return Err(format!(
                "The custom resource's function failed before it sent a response: {error}"
            ));
        }
        (None, _) => {
            responses.forget(&url);
            // (memory)
            return Err(format!(
                "CloudFormation did not receive a response from your Custom Resource. Please check your logs for requestId [{request_id}]."
            ));
        }
    };
    answered(&answer, &request_id, old)
}

/// The function invoked: `None` if it ran (its answer comes on the response URL), or its error when it failed (it may still have
/// answered first).
async fn invoke(
    rc: &Rc,
    function: &str,
    event: &Value,
) -> Result<Option<String>, aws_wire::ApiError> {
    let body = json!({"FunctionName": function, "InvocationType": "RequestResponse",
        "Payload": aws_core::base64::encode(event.to_string().as_bytes())});
    let out = rc.call("lambda", "Invoke", body).await?;
    Ok(out
        .get("FunctionError")
        .and_then(Value::as_str)
        .map(|kind| {
            let payload = out
                .at("Payload")
                .as_str()
                .and_then(aws_core::base64::decode)
                .unwrap_or_default();
            format!("{kind}: {}", String::from_utf8_lossy(&payload))
        }))
}

/// A handler's answer as the resource's outcome.
fn answered(answer: &Value, request_id: &str, old: Option<&Res>) -> Done {
    if answer.at("Status") != "SUCCESS" {
        let why = answer
            .at("Reason")
            .as_str()
            .unwrap_or("See the details in CloudWatch Log Stream");
        // (memory)
        return Err(format!(
            "Received response status [FAILED] from custom resource. Message returned: {why} (RequestId: {request_id})"
        ));
    }
    let physical = answer
        .at("PhysicalResourceId")
        .as_str()
        .map(str::to_owned)
        .or_else(|| old.and_then(|o| o.physical.clone()))
        .unwrap_or_else(|| request_id.to_owned());
    let mut m = Made::new(physical);
    if let Some(Value::Object(data)) = answer.get("Data") {
        m.attrs = data.clone();
    }
    Ok(m)
}

/// Data flattened to dotted paths (`a.b.0.c`), as `Custom::AWS` and the provider framework expose a response.
pub(crate) fn flatten(v: &Value, prefix: &str, out: &mut Map<String, Value>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(x, &key, out);
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                let key = if prefix.is_empty() {
                    i.to_string()
                } else {
                    format!("{prefix}.{i}")
                };
                flatten(x, &key, out);
            }
        }
        Value::String(s) => {
            out.insert(prefix.to_owned(), Value::String(s.clone()));
        }
        Value::Null => {}
        other => {
            out.insert(prefix.to_owned(), Value::String(other.to_string()));
        }
    }
}
