//! `AWS::Serverless-2016-10-31` (SAM): `Globals` folded in, then each serverless resource expanded into the CloudFormation resources
//! SAM's translator makes (functions with their roles, versions, aliases, URLs and events; APIs with their deployment and stage;
//! simple tables, layers and state machines), with the logical ids SAM gives them so `!GetAtt FnRole.Arn` and `!Ref ServerlessRestApi`
//! keep working. What SAM can make that doze-aws does not serve (applications from the Serverless Application Repository, GraphQL
//! APIs, connectors, event sources such as IoT or MSK) is refused by name.

use serde_json::{Map, Value, json};

use aws_wire::ApiError;

use super::sam_api;
use super::sam_events::{self, Out};
use crate::errors::validation;
use crate::j::J;
use crate::tmpl::eval::Pseudo;

/// Expands the SAM resources of `t` into plain CloudFormation.
///
/// # Errors
/// What SAM refuses, and what doze-aws does not serve.
pub(crate) fn apply(t: &mut Value, pseudo: &Pseudo) -> Result<(), ApiError> {
    let globals = t
        .as_object_mut()
        .and_then(|o| o.remove("Globals"))
        .unwrap_or(Value::Null);
    let Some(resources) = t.get("Resources").and_then(Value::as_object).cloned() else {
        return Ok(());
    };
    let mut out = Out::default();
    for (name, r) in &resources {
        let ty = crate::tmpl::type_of(r).to_owned();
        let Some(kind) = ty.strip_prefix("AWS::Serverless::") else {
            out.put(name, r.clone());
            continue;
        };
        let mut r = r.clone();
        if let Some(g) = globals.get(kind) {
            let props = r.get("Properties").cloned().unwrap_or(json!({}));
            r.put("Properties", merge(g, &props));
        }
        match kind {
            "Function" => function(name, &r, &resources, &mut out, pseudo)?,
            "Api" => sam_api::rest_api(name, &r, &resources, &mut out)?,
            "HttpApi" => sam_api::http_api(name, &r, &resources, &mut out)?,
            "SimpleTable" => simple_table(name, &r, &mut out),
            "LayerVersion" => layer(name, &r, &mut out),
            "StateMachine" => super::sam_sfn::state_machine(name, &r, &mut out)?,
            other => {
                // (memory) SAM's own wording differs; this names what doze-aws does not serve.
                return Err(validation(format!(
                    "Transform AWS::Serverless-2016-10-31 failed with: Resource {name} of type AWS::Serverless::{other} is not served by doze-aws"
                )));
            }
        }
    }
    sam_api::implicit_apis(&resources, &mut out)?;
    let mut resources = out.resources;
    rewrite_refs(&mut resources, &out.aliases);
    t.put("Resources", Value::Object(resources));
    if let Some(outputs) = t.get_mut("Outputs") {
        rewrite_refs_in(outputs, &out.aliases);
    }
    Ok(())
}

/// `Globals` under a resource's own properties: objects merged (the resource wins), lists joined (the globals first).
pub(crate) fn merge(global: &Value, local: &Value) -> Value {
    match (global, local) {
        (Value::Object(g), Value::Object(l)) => {
            let mut out = g.clone();
            for (k, v) in l {
                let merged = match out.get(k) {
                    Some(gv) => merge(gv, v),
                    None => v.clone(),
                };
                out.insert(k.clone(), merged);
            }
            Value::Object(out)
        }
        (Value::Array(g), Value::Array(l)) => Value::Array(g.iter().chain(l).cloned().collect()),
        (_, l) => l.clone(),
    }
}

/// SAM's references to what it made: `Fn.Alias`, `Fn.Version`, `Api.Stage`, `Api.Deployment` in `Ref`s.
fn rewrite_refs(resources: &mut Map<String, Value>, aliases: &Map<String, Value>) {
    for r in resources.values_mut() {
        rewrite_refs_in(r, aliases);
    }
}

fn rewrite_refs_in(v: &mut Value, aliases: &Map<String, Value>) {
    match v {
        Value::Object(o) => {
            if o.len() == 1
                && let Some(Value::String(target)) = o.get("Ref")
                && let Some(to) = aliases.get(target)
            {
                o.insert("Ref".into(), to.clone());
                return;
            }
            for x in o.values_mut() {
                rewrite_refs_in(x, aliases);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| rewrite_refs_in(x, aliases)),
        _ => {}
    }
}

/// Ten hex digits of a value's SHA-256, as SAM suffixes the logical ids of versions and deployments so a change makes a new one.
pub(crate) fn hash10(v: &Value) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(v.to_string().as_bytes());
    aws_core::hex::encode(&digest).chars().take(10).collect()
}

/// The resource attributes a SAM resource passes on to what it expands to.
pub(crate) fn passed(r: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    for k in [
        "DependsOn",
        "Condition",
        "DeletionPolicy",
        "UpdateReplacePolicy",
        "Metadata",
    ] {
        if let Some(v) = r.get(k) {
            out.insert(k.into(), v.clone());
        }
    }
    out
}

/// A resource of `ty` with `props` and the attributes of the SAM resource it comes from.
pub(crate) fn resource(ty: &str, props: Value, from: &Value) -> Value {
    let mut out = passed(from);
    out.insert("Type".into(), ty.into());
    out.insert("Properties".into(), props);
    Value::Object(out)
}

/// SAM's tags as CloudFormation's list, with `lambda:createdBy: SAM` as SAM adds it to functions.
pub(crate) fn tag_list(p: &Value, created_by: bool) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    if created_by {
        out.push(json!({"Key": "lambda:createdBy", "Value": "SAM"}));
    }
    if let Some(Value::Object(tags)) = p.get("Tags") {
        out.extend(tags.iter().map(|(k, v)| json!({"Key": k, "Value": v})));
    }
    out
}

/// `CodeUri` / `InlineCode` / `ImageUri` as a function's `Code`.
fn code(name: &str, p: &Value) -> Result<Value, ApiError> {
    if let Some(inline) = p.get("InlineCode") {
        return Ok(json!({"ZipFile": inline}));
    }
    if let Some(image) = p.get("ImageUri") {
        return Ok(json!({"ImageUri": image}));
    }
    location(name, p.get("CodeUri"), "CodeUri")
}

/// An S3 location written as `s3://bucket/key`, as `{Bucket, Key, Version}`, or as an absolute local directory (run in place).
pub(crate) fn location(name: &str, uri: Option<&Value>, what: &str) -> Result<Value, ApiError> {
    match uri {
        Some(Value::String(s)) if s.starts_with("s3://") => {
            let (bucket, key) = s
                .trim_start_matches("s3://")
                .split_once('/')
                .unwrap_or((s, ""));
            Ok(json!({"S3Bucket": bucket, "S3Key": key}))
        }
        Some(Value::String(s)) if s.starts_with('/') => {
            Ok(json!({"S3Bucket": "_local_", "S3Key": s}))
        }
        Some(Value::Object(o)) => {
            let mut out = json!({"S3Bucket": o.get("Bucket"), "S3Key": o.get("Key")});
            if let Some(v) = o.get("Version") {
                out.put("S3ObjectVersion", v.clone());
            }
            Ok(out)
        }
        Some(other) if other.is_object() || other.get("Fn::Sub").is_some() => {
            Ok(json!({"S3Bucket": "_local_", "S3Key": other}))
        }
        // (memory)
        _ => Err(validation(format!(
            "Transform AWS::Serverless-2016-10-31 failed with: Resource {name}: '{what}' must be an S3 location (package the template with `sam package`) or an absolute local path"
        ))),
    }
}

fn function(
    name: &str,
    r: &Value,
    all: &Map<String, Value>,
    out: &mut Out,
    pseudo: &Pseudo,
) -> Result<(), ApiError> {
    let p = r.get("Properties").cloned().unwrap_or(json!({}));
    let mut f = json!({"Code": code(name, &p)?, "Tags": tag_list(&p, true)});
    for k in [
        "FunctionName",
        "Handler",
        "Runtime",
        "Description",
        "MemorySize",
        "Timeout",
        "Environment",
        "VpcConfig",
        "Layers",
        "KmsKeyArn",
        "Architectures",
        "EphemeralStorage",
        "ReservedConcurrentExecutions",
        "FileSystemConfigs",
        "ImageConfig",
        "SnapStart",
        "LoggingConfig",
        "RuntimeManagementConfig",
        "RecursiveLoop",
        "PackageType",
        "CodeSigningConfigArn",
    ] {
        if let Some(v) = p.get(k) {
            f.put(if k == "KmsKeyArn" { "KMSKeyArn" } else { k }, v.clone());
        }
    }
    if p.get("ImageUri").is_some() {
        f.put("PackageType", "Image".into());
    }
    if let Some(mode) = p.get("Tracing") {
        f.put("TracingConfig", json!({"Mode": mode}));
    }
    if let Some(dlq) = p.get("DeadLetterQueue") {
        f.put(
            "DeadLetterConfig",
            json!({"TargetArn": dlq.at("TargetArn")}),
        );
    }
    let role = match p.get("Role") {
        Some(role) => role.clone(),
        None => {
            let role_name = format!("{name}Role");
            out.put(
                &role_name,
                super::sam_role::execution_role(&p, r, "lambda.amazonaws.com")?,
            );
            json!({"Fn::GetAtt": [role_name, "Arn"]})
        }
    };
    f.put("Role", role);
    out.put(name, resource("AWS::Lambda::Function", f, r));
    sam_events::function_extras(name, &p, r, out, pseudo);
    sam_events::function_events(name, &p, r, all, out)
}

fn simple_table(name: &str, r: &Value, out: &mut Out) {
    let p = r.get("Properties").cloned().unwrap_or(json!({}));
    let key = p
        .get("PrimaryKey")
        .cloned()
        .unwrap_or(json!({"Name": "id", "Type": "String"}));
    let ty = match key.at("Type").as_str() {
        Some("Number") => "N",
        Some("Binary") => "B",
        _ => "S",
    };
    let mut t = json!({"AttributeDefinitions": [{"AttributeName": key.at("Name"), "AttributeType": ty}],
        "KeySchema": [{"AttributeName": key.at("Name"), "KeyType": "HASH"}]});
    match p.get("ProvisionedThroughput") {
        Some(pt) => t.put("ProvisionedThroughput", pt.clone()),
        None => t.put("BillingMode", "PAY_PER_REQUEST".into()),
    }
    for k in [
        "TableName",
        "SSESpecification",
        "PointInTimeRecoverySpecification",
    ] {
        if let Some(v) = p.get(k) {
            t.put(k, v.clone());
        }
    }
    let tags = tag_list(&p, false);
    if !tags.is_empty() {
        t.put("Tags", Value::Array(tags));
    }
    out.put(name, resource("AWS::DynamoDB::Table", t, r));
}

fn layer(name: &str, r: &Value, out: &mut Out) {
    let p = r.get("Properties").cloned().unwrap_or(json!({}));
    let content = location(name, p.get("ContentUri"), "ContentUri").unwrap_or(Value::Null);
    let mut l = json!({"Content": content});
    for k in [
        "LayerName",
        "Description",
        "CompatibleRuntimes",
        "CompatibleArchitectures",
        "LicenseInfo",
    ] {
        if let Some(v) = p.get(k) {
            l.put(k, v.clone());
        }
    }
    let mut res = resource("AWS::Lambda::LayerVersion", l, r);
    match p.at("RetentionPolicy").as_str() {
        Some("Retain") => res.put("DeletionPolicy", "Retain".into()),
        Some("Delete") => res.put("DeletionPolicy", "Delete".into()),
        _ => {}
    }
    out.put(name, res);
}
