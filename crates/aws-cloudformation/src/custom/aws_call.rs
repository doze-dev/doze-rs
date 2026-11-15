//! `Custom::AWS` (the CDK's `AwsCustomResource`): each lifecycle event is one SDK call (`{service, action, parameters,
//! physicalResourceId, outputPaths, ignoreErrorCodesMatching}`), made here through the named service's own operation. SDK v2 and v3
//! spellings are both read (`SSM`/`@aws-sdk/client-ssm`, `putParameter`/`PutParameterCommand`).

use serde_json::{Map, Value, json};

use super::{Request, flatten};
use crate::j::J;
use crate::model::Res;
use crate::res::{Done, Made, Rc};

/// The signing name a CDK service name means.
fn service(name: &str) -> Option<&'static str> {
    let n = name
        .trim_start_matches("@aws-sdk/client-")
        .to_ascii_lowercase()
        .replace('-', "");
    Some(match n.as_str() {
        "ssm" => "ssm",
        "s3" => "s3",
        "sqs" => "sqs",
        "sns" => "sns",
        "dynamodb" => "dynamodb",
        "lambda" => "lambda",
        "cloudwatchlogs" | "logs" => "logs",
        "cloudwatch" => "monitoring",
        "secretsmanager" => "secretsmanager",
        "kms" => "kms",
        "iam" => "iam",
        "sts" => "sts",
        "kinesis" => "kinesis",
        "eventbridge" | "cloudwatchevents" => "events",
        "sfn" | "stepfunctions" => "states",
        _ => return None,
    })
}

/// `putParameter` or `PutParameterCommand` as `PutParameter`.
fn operation(action: &str) -> String {
    let a = action.strip_suffix("Command").unwrap_or(action);
    let mut out = a.to_owned();
    if let Some(f) = out.get_mut(..1) {
        f.make_ascii_uppercase();
    }
    out
}

/// `PHYSICAL:RESOURCEID:` (the CDK's placeholder) replaced by the physical id.
fn substitute(v: &Value, physical: &str) -> Value {
    match v {
        Value::String(s) if s == "PHYSICAL:RESOURCEID:" => Value::String(physical.to_owned()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| (k.clone(), substitute(x, physical)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(|x| substitute(x, physical)).collect()),
        other => other.clone(),
    }
}

pub(super) async fn run(rc: &Rc, req: Request, old: Option<&Res>, p: &Value) -> Done {
    let key = match req {
        Request::Create => "Create",
        Request::Update => "Update",
        Request::Delete => "Delete",
    };
    let physical_before = old.and_then(|o| o.physical.clone()).unwrap_or_default();
    let call = match p.get(key) {
        Some(Value::String(s)) => serde_json::from_str::<Value>(s).unwrap_or(Value::Null),
        Some(v) => v.clone(),
        // No call for this event: nothing to do (an update with no Update call keeps the resource).
        None => {
            let mut m = Made::new(if physical_before.is_empty() {
                rc.logical.clone()
            } else {
                physical_before
            });
            m.attrs = old.map(Res::attrs_map).unwrap_or_default();
            return Ok(m);
        }
    };
    let call = substitute(&call, &physical_before);
    let svc_name = call.at("service").as_str().unwrap_or_default();
    let Some(svc) = service(svc_name) else {
        return Err(format!(
            "Custom::AWS: service {svc_name} is not served by doze-aws"
        ));
    };
    let op = operation(call.at("action").as_str().unwrap_or_default());
    let params = call.get("parameters").cloned().unwrap_or(json!({}));
    let result = if svc == "s3" {
        rc.rest(svc, &op, params).await
    } else {
        rc.call(svc, &op, params).await
    };
    let response = match result {
        Ok(r) => r,
        Err(e) => ignored(rc, &call, &e)?,
    };
    Ok(made(rc, &call, &response, physical_before))
}

/// An error the call says to ignore (`ignoreErrorCodesMatching`) as an empty response; any other as the resource's failure.
fn ignored(rc: &Rc, call: &Value, e: &aws_wire::ApiError) -> Result<Value, String> {
    let ignore = call
        .at("ignoreErrorCodesMatching")
        .as_str()
        .unwrap_or_default();
    if !ignore.is_empty() && regex_lite::Regex::new(ignore).is_ok_and(|re| re.is_match(&e.code)) {
        return Ok(json!({}));
    }
    // (memory)
    Err(format!(
        "Received response status [FAILED] from custom resource. Message returned: {} (RequestId: {})",
        e.message, rc.token
    ))
}

/// The resource as the call's response makes it: its physical id and the response's output paths.
fn made(rc: &Rc, call: &Value, response: &Value, physical_before: String) -> Made {
    let mut flat = Map::new();
    flatten(response, "", &mut flat);
    let physical = match call.at("physicalResourceId") {
        Value::Object(o) if o.contains_key("id") => o
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        Value::Object(o) if o.contains_key("responsePath") => {
            let path = o
                .get("responsePath")
                .and_then(Value::as_str)
                .unwrap_or_default();
            flat.get(path)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        }
        _ if !physical_before.is_empty() => physical_before,
        _ => rc.logical.clone(),
    };
    let wanted: Vec<String> = match (call.get("outputPaths"), call.get("outputPath")) {
        (Some(Value::Array(a)), _) => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect(),
        (_, Some(Value::String(s))) => vec![s.clone()],
        _ => Vec::new(),
    };
    if !wanted.is_empty() {
        flat.retain(|k, _| {
            wanted
                .iter()
                .any(|w| k == w || k.starts_with(&format!("{w}.")))
        });
    }
    let mut m = Made::new(physical);
    m.attrs = flat;
    m
}
