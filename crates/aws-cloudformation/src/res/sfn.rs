//! `AWS::StepFunctions::StateMachine` (`Definition`, `DefinitionString` or `DefinitionS3Location` from the local S3, with
//! `DefinitionSubstitutions`), `Activity`, `StateMachineVersion` and `StateMachineAlias`, through Step Functions' own operations
//! (signing name `states`; its members are lower camel case). When Step Functions is not running, the resource fails and says so.

use serde_json::{Value, json};

use super::logs::camel;
use super::{Done, Made, Rc, document, gone, tag_list, text};
use crate::j::J;
use crate::model::Res;

pub(crate) const SVC: &str = "states";

async fn call(rc: &Rc, op: &str, body: Value) -> Result<Value, String> {
    rc.call(SVC, op, body).await.map_err(|e| rc.fail(SVC, &e))
}

fn tags(rc: &Rc, p: &Value) -> Value {
    Value::Array(
        tag_list(rc, p)
            .into_iter()
            .map(|(k, v)| json!({"key": k, "value": v}))
            .collect(),
    )
}

/// The definition as text, with its substitutions made.
async fn definition(rc: &Rc, p: &Value) -> Result<String, String> {
    let mut text_def = match (
        p.get("Definition"),
        document(p, "DefinitionString"),
        p.get("DefinitionS3Location"),
    ) {
        (Some(d), _, _) => d.to_string(),
        (None, Some(s), _) => s,
        (None, None, Some(loc)) => {
            let (bucket, key) = (
                loc.at("Bucket").as_str().unwrap_or_default(),
                loc.at("Key").as_str().unwrap_or_default(),
            );
            let bytes = crate::transform::include::object(
                rc.peers.as_deref(),
                &rc.caller,
                bucket,
                key,
                1 << 20,
            )
            .await
            .map_err(|e| {
                rc.handler(
                    &format!("DefinitionS3Location s3://{bucket}/{key}: {e}"),
                    "NotFound",
                )
            })?;
            let raw = String::from_utf8_lossy(&bytes).into_owned();
            // A definition in S3 may be YAML; Step Functions takes JSON.
            crate::tmpl::read_snippet(&raw)
                .map(|v| v.to_string())
                .unwrap_or(raw)
        }
        // (memory)
        (None, None, None) => {
            return Err(rc.handler(
                "Either Definition, DefinitionString or DefinitionS3Location must be specified",
                "InvalidRequest",
            ));
        }
    };
    if let Some(Value::Object(subs)) = p.get("DefinitionSubstitutions") {
        for (k, v) in subs {
            let value = v.as_str().map_or_else(|| v.to_string(), str::to_owned);
            text_def = text_def.replace(&format!("${{{k}}}"), &value);
        }
    }
    Ok(text_def)
}

fn config(p: &Value) -> Value {
    let mut b = json!({});
    for (from, to) in [
        ("LoggingConfiguration", "loggingConfiguration"),
        ("TracingConfiguration", "tracingConfiguration"),
        ("EncryptionConfiguration", "encryptionConfiguration"),
    ] {
        if let Some(v) = p.get(from) {
            b.put(to, camel(v));
        }
    }
    b
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    match rc.ty.as_str() {
        "AWS::StepFunctions::StateMachine" => {
            let name =
                text(p, "StateMachineName").unwrap_or_else(|| super::generated_name(rc, 80, false));
            let mut body = config(p);
            for (k, v) in [
                ("name", json!(name)),
                ("definition", json!(definition(rc, p).await?)),
                ("roleArn", json!(text(p, "RoleArn"))),
                (
                    "type",
                    json!(text(p, "StateMachineType").unwrap_or_else(|| "STANDARD".into())),
                ),
                ("tags", tags(rc, p)),
            ] {
                body.put(k, v);
            }
            let made = call(rc, "CreateStateMachine", body).await?;
            let arn = made
                .at("stateMachineArn")
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let described =
                call(rc, "DescribeStateMachine", json!({"stateMachineArn": arn})).await?;
            Ok(Made::new(&arn)
                .with("Arn", arn.clone())
                .with("Name", name)
                .with("StateMachineRevisionId", described.at("revisionId").clone()))
        }
        "AWS::StepFunctions::Activity" => {
            let name = text(p, "Name").unwrap_or_default();
            let made = call(
                rc,
                "CreateActivity",
                json!({"name": name, "tags": tags(rc, p)}),
            )
            .await?;
            let arn = made
                .at("activityArn")
                .as_str()
                .unwrap_or_default()
                .to_owned();
            Ok(Made::new(&arn).with("Arn", arn.clone()).with("Name", name))
        }
        "AWS::StepFunctions::StateMachineVersion" => {
            let body = json!({"stateMachineArn": text(p, "StateMachineArn"), "revisionId": text(p, "StateMachineRevisionId"), "description": text(p, "Description")});
            let made = call(rc, "PublishStateMachineVersion", body).await?;
            let arn = made
                .at("stateMachineVersionArn")
                .as_str()
                .unwrap_or_default()
                .to_owned();
            Ok(Made::new(&arn).with("Arn", arn.clone()))
        }
        "AWS::StepFunctions::StateMachineAlias" => {
            let made = call(rc, "CreateStateMachineAlias", alias_body(rc, p)).await?;
            let arn = made
                .at("stateMachineAliasArn")
                .as_str()
                .unwrap_or_default()
                .to_owned();
            Ok(Made::new(&arn).with("Arn", arn.clone()))
        }
        other => Err(format!("{other} is not a type doze-aws makes")),
    }
}

/// An alias's routing: `RoutingConfiguration` as given, or all traffic to `DeploymentPreference`'s version (the shift is instant).
fn alias_body(rc: &Rc, p: &Value) -> Value {
    let routing = match (p.get("RoutingConfiguration"), p.get("DeploymentPreference")) {
        (Some(r), _) => camel(r),
        (None, Some(d)) => {
            json!([{"stateMachineVersionArn": d.at("StateMachineVersionArn"), "weight": 100}])
        }
        (None, None) => json!([]),
    };
    let name = text(p, "Name").unwrap_or_else(|| super::generated_name(rc, 80, false));
    json!({"name": name, "description": text(p, "Description"), "routingConfiguration": routing})
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let arn = old.physical.clone().unwrap_or_default();
    match rc.ty.as_str() {
        "AWS::StepFunctions::StateMachine" => {
            let mut body = config(p);
            body.put("stateMachineArn", arn.clone().into());
            body.put("definition", definition(rc, p).await?.into());
            body.put("roleArn", json!(text(p, "RoleArn")));
            call(rc, "UpdateStateMachine", body).await?;
        }
        "AWS::StepFunctions::StateMachineAlias" => {
            let mut body = alias_body(rc, p);
            if let Some(o) = body.as_object_mut() {
                o.remove("name");
            }
            body.put("stateMachineAliasArn", arn.clone().into());
            call(rc, "UpdateStateMachineAlias", body).await?;
        }
        _ => {}
    }
    let mut m = Made::new(&arn);
    m.attrs = old.attrs_map();
    Ok(m)
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let arn = res.physical.clone().unwrap_or_default();
    let (op, body) = match rc.ty.as_str() {
        "AWS::StepFunctions::StateMachine" => {
            ("DeleteStateMachine", json!({"stateMachineArn": arn}))
        }
        "AWS::StepFunctions::Activity" => ("DeleteActivity", json!({"activityArn": arn})),
        "AWS::StepFunctions::StateMachineVersion" => (
            "DeleteStateMachineVersion",
            json!({"stateMachineVersionArn": arn}),
        ),
        "AWS::StepFunctions::StateMachineAlias" => (
            "DeleteStateMachineAlias",
            json!({"stateMachineAliasArn": arn}),
        ),
        _ => return Ok(()),
    };
    match rc.call(SVC, op, body).await {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) || e.message.starts_with("no service ") => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}
