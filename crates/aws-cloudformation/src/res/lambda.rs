//! `AWS::Lambda::Function`: made with Lambda's own `CreateFunction` (code from S3, from a local directory with `S3Bucket: _local_`, or
//! inline `ZipFile` source packaged as `index.js` or `index.py` with the `cfn-response` module beside it, as CloudFormation packages
//! it), updated in place with `UpdateFunctionConfiguration` and `UpdateFunctionCode`. The other Lambda types are in `lambda_more`.

use serde_json::{Map, Value, json};

use super::{Done, Made, Rc, gone, tag_list, text};
use crate::j::J;
use crate::model::Res;

pub(super) const SVC: &str = "lambda";

/// The cfn-response modules CloudFormation adds to inline code.
const CFN_RESPONSE_PY: &str = include_str!("cfnresponse.py");
const CFN_RESPONSE_JS: &str = include_str!("cfn-response.js");

/// Configuration members passed through to `CreateFunction` and `UpdateFunctionConfiguration` as written.
const CONFIG: &[&str] = &[
    "Runtime",
    "Role",
    "Handler",
    "Description",
    "Timeout",
    "MemorySize",
    "VpcConfig",
    "DeadLetterConfig",
    "Environment",
    "KMSKeyArn",
    "TracingConfig",
    "Layers",
    "FileSystemConfigs",
    "ImageConfig",
    "Architectures",
    "EphemeralStorage",
    "SnapStart",
    "LoggingConfig",
];

fn config(p: &Value) -> Map<String, Value> {
    CONFIG
        .iter()
        .filter_map(|k| {
            p.get(*k)
                .filter(|v| !v.is_null())
                .map(|v| ((*k).to_owned(), v.clone()))
        })
        .collect()
}

/// The `Code` member: S3 and image code as given, inline source as a one-file package.
fn code(p: &Value) -> Value {
    let c = p.at("Code");
    let Some(source) = c.get("ZipFile").and_then(Value::as_str) else {
        let mut out = c.clone();
        if let Some(o) = out.as_object_mut() {
            o.retain(|k, _| {
                matches!(
                    k.as_str(),
                    "S3Bucket" | "S3Key" | "S3ObjectVersion" | "ImageUri"
                )
            });
        }
        return out;
    };
    let runtime = text(p, "Runtime").unwrap_or_default();
    let zip = if runtime.starts_with("python") {
        crate::zip::stored(&[
            ("index.py", source.as_bytes()),
            ("cfnresponse.py", CFN_RESPONSE_PY.as_bytes()),
        ])
    } else {
        crate::zip::stored(&[
            ("index.js", source.as_bytes()),
            (
                "node_modules/cfn-response/index.js",
                CFN_RESPONSE_JS.as_bytes(),
            ),
        ])
    };
    json!({"ZipFile": aws_core::base64::encode(&zip)})
}

fn arn(rc: &Rc, name: &str) -> String {
    format!(
        "arn:aws:lambda:{}:{}:function:{name}",
        rc.caller.region, rc.caller.account_id
    )
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    if rc.ty != "AWS::Lambda::Function" {
        return super::lambda_more::create(rc, p).await;
    }
    let name = text(p, "FunctionName").unwrap_or_else(|| super::generated_name(rc, 64, false));
    if rc
        .call(SVC, "GetFunction", json!({"FunctionName": name}))
        .await
        .is_ok()
    {
        return Err(rc.taken(&name));
    }
    let mut body = Value::Object(config(p));
    body.put("FunctionName", name.clone().into());
    body.put("Code", code(p));
    if let Some(t) = text(p, "PackageType") {
        body.put("PackageType", t.into());
    }
    let tags: Map<String, Value> = tag_list(rc, p)
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    if !tags.is_empty() {
        body.put("Tags", Value::Object(tags));
    }
    let made = rc
        .call(SVC, "CreateFunction", body)
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    settings(rc, &name, &Value::Null, p).await?;
    Ok(described(&made, &name))
}

fn described(made: &Value, name: &str) -> Made {
    let mut m = Made::new(name).with("Arn", made.at("FunctionArn").clone());
    if let Some(s) = made.get("SnapStart") {
        m = m.with("SnapStartResponse", s.clone());
        if let Some(v) = s.get("ApplyOn") {
            m = m.with("SnapStartResponse.ApplyOn", v.clone());
        }
        if let Some(v) = s.get("OptimizationStatus") {
            m = m.with("SnapStartResponse.OptimizationStatus", v.clone());
        }
    }
    m
}

/// Reserved concurrency and recursion settings, which have operations of their own.
async fn settings(rc: &Rc, name: &str, before: &Value, p: &Value) -> Result<(), String> {
    let (then, now) = (
        text(before, "ReservedConcurrentExecutions"),
        text(p, "ReservedConcurrentExecutions"),
    );
    if then != now {
        let r = match now {
            Some(n) => {
                let body = json!({"FunctionName": name, "ReservedConcurrentExecutions": n});
                rc.call(SVC, "PutFunctionConcurrency", body).await
            }
            None => {
                rc.call(
                    SVC,
                    "DeleteFunctionConcurrency",
                    json!({"FunctionName": name}),
                )
                .await
            }
        };
        r.map_err(|e| rc.fail(SVC, &e))?;
    }
    if let Some(loop_setting) =
        text(p, "RecursiveLoop").filter(|r| Some(r) != text(before, "RecursiveLoop").as_ref())
    {
        let body = json!({"FunctionName": name, "RecursiveLoop": loop_setting});
        rc.call(SVC, "PutFunctionRecursionConfig", body)
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
    }
    if let Some(m) = p
        .get("RuntimeManagementConfig")
        .filter(|m| Some(*m) != before.get("RuntimeManagementConfig"))
    {
        let mut body = m.clone();
        body.put("FunctionName", name.into());
        rc.call(SVC, "PutRuntimeManagementConfig", body)
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
    }
    Ok(())
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    if rc.ty != "AWS::Lambda::Function" {
        return super::lambda_more::update(rc, old, p).await;
    }
    let name = old.physical.clone().unwrap_or_default();
    let before = old.props_value();
    if config(&before) != config(p) {
        let mut body = Value::Object(config(p));
        body.put("FunctionName", name.clone().into());
        rc.call(SVC, "UpdateFunctionConfiguration", body)
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
    }
    if before.get("Code") != p.get("Code") {
        let mut body = code(p);
        body.put("FunctionName", name.clone().into());
        rc.call(SVC, "UpdateFunctionCode", body)
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
    }
    settings(rc, &name, &before, p).await?;
    let (then, now) = (tag_list(rc, &before), tag_list(rc, p));
    if then != now {
        let resource = arn(rc, &name);
        let removed: Vec<&String> = then
            .iter()
            .map(|(k, _)| k)
            .filter(|k| !now.iter().any(|(n, _)| n == *k))
            .collect();
        if !removed.is_empty() {
            rc.call(
                SVC,
                "UntagResource",
                json!({"Resource": resource, "TagKeys": removed}),
            )
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
        }
        let tags: Map<String, Value> = now
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect();
        if !tags.is_empty() {
            rc.call(
                SVC,
                "TagResource",
                json!({"Resource": resource, "Tags": tags}),
            )
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
        }
    }
    let got = rc
        .call(
            SVC,
            "GetFunctionConfiguration",
            json!({"FunctionName": name}),
        )
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    Ok(described(&got, &name))
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    if rc.ty != "AWS::Lambda::Function" {
        return super::lambda_more::delete(rc, res).await;
    }
    let name = res.physical.clone().unwrap_or_default();
    match rc
        .call(SVC, "DeleteFunction", json!({"FunctionName": name}))
        .await
    {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}
