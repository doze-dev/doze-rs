//! `AWS::Events::Rule` (with its targets), `EventBus`, `EventBusPolicy`, `Archive`, `Connection` and `ApiDestination`.

use serde_json::{Value, json};

use super::{Done, Made, Rc, document, gone, tag_list, text};
use crate::j::J;
use crate::model::Res;

const SVC: &str = "events";

async fn call(rc: &Rc, op: &str, body: Value) -> Result<Value, String> {
    rc.call(SVC, op, body).await.map_err(|e| rc.fail(SVC, &e))
}

fn pick(p: &Value, keys: &[&str]) -> Value {
    let mut out = json!({});
    for k in keys {
        if let Some(v) = p.get(*k).filter(|v| !v.is_null()) {
            out.put(k, v.clone());
        }
    }
    out
}

const BUS_POLICY: &[&str] = &[
    "EventBusName",
    "StatementId",
    "Action",
    "Principal",
    "Condition",
];

fn bus_of(p: &Value) -> Option<String> {
    text(p, "EventBusName").filter(|b| b != "default")
}

/// A rule's physical id: its name, or `bus|name` on a bus of its own (memory).
fn rule_id(p: &Value, name: &str) -> String {
    match bus_of(p) {
        Some(bus) => format!("{bus}|{name}"),
        None => name.to_owned(),
    }
}

fn targets(p: &Value) -> Vec<Value> {
    p.get("Targets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

async fn put_rule(rc: &Rc, name: &str, p: &Value) -> Result<String, String> {
    let mut body = pick(
        p,
        &[
            "ScheduleExpression",
            "State",
            "Description",
            "RoleArn",
            "EventBusName",
        ],
    );
    body.put("Name", name.into());
    if let Some(pattern) = document(p, "EventPattern") {
        body.put("EventPattern", pattern.into());
    }
    let tags: Vec<Value> = tag_list(rc, p)
        .into_iter()
        .map(|(k, v)| json!({"Key": k, "Value": v}))
        .collect();
    if !tags.is_empty() {
        body.put("Tags", Value::Array(tags));
    }
    let made = call(rc, "PutRule", body).await?;
    Ok(made.at("RuleArn").as_str().unwrap_or_default().to_owned())
}

async fn retarget(rc: &Rc, name: &str, before: &Value, p: &Value) -> Result<(), String> {
    let now = targets(p);
    let gone_ids: Vec<Value> = targets(before)
        .iter()
        .filter_map(|t| t.get("Id").cloned())
        .filter(|id| !now.iter().any(|n| n.get("Id") == Some(id)))
        .collect();
    let mut on = json!({"Rule": name});
    if let Some(bus) = bus_of(p).or_else(|| bus_of(before)) {
        on.put("EventBusName", bus.into());
    }
    if !gone_ids.is_empty() {
        let mut body = on.clone();
        body.put("Ids", Value::Array(gone_ids));
        call(rc, "RemoveTargets", body).await?;
    }
    if !now.is_empty() && now != targets(before) {
        let mut body = on;
        body.put("Targets", Value::Array(now));
        call(rc, "PutTargets", body).await?;
    }
    Ok(())
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    match rc.ty.as_str() {
        "AWS::Events::Rule" => {
            let name = text(p, "Name").unwrap_or_else(|| super::generated_name(rc, 64, false));
            let arn = put_rule(rc, &name, p).await?;
            retarget(rc, &name, &Value::Null, p).await?;
            Ok(Made::new(rule_id(p, &name)).with("Arn", arn))
        }
        "AWS::Events::EventBus" => {
            let name = text(p, "Name").unwrap_or_default();
            let mut body = pick(
                p,
                &[
                    "EventSourceName",
                    "Description",
                    "KmsKeyIdentifier",
                    "DeadLetterConfig",
                ],
            );
            body.put("Name", name.clone().into());
            let made = call(rc, "CreateEventBus", body).await?;
            if let Some(policy) = document(p, "Policy") {
                call(
                    rc,
                    "PutPermission",
                    json!({"EventBusName": name, "Policy": policy}),
                )
                .await?;
            }
            Ok(Made::new(&name)
                .with("Arn", made.at("EventBusArn").clone())
                .with("Name", name.clone()))
        }
        _ => create_more(rc, p).await,
    }
}

async fn create_more(rc: &Rc, p: &Value) -> Done {
    match rc.ty.as_str() {
        "AWS::Events::EventBusPolicy" => {
            let id = text(p, "StatementId").unwrap_or_default();
            let mut body = pick(p, BUS_POLICY);
            if let Some(s) = p.get("Statement") {
                body = pick(p, &["EventBusName"]);
                body.put(
                    "Policy",
                    json!({"Version": "2012-10-17", "Statement": [s]})
                        .to_string()
                        .into(),
                );
            }
            call(rc, "PutPermission", body).await?;
            Ok(Made::new(id))
        }
        "AWS::Events::Archive" => {
            let name =
                text(p, "ArchiveName").unwrap_or_else(|| super::generated_name(rc, 48, false));
            let mut body = pick(p, &["SourceArn", "Description", "RetentionDays"]);
            body.put("ArchiveName", name.clone().into());
            if let Some(pattern) = document(p, "EventPattern") {
                body.put("EventPattern", pattern.into());
            }
            let made = call(rc, "CreateArchive", body).await?;
            Ok(Made::new(&name)
                .with("Arn", made.at("ArchiveArn").clone())
                .with("ArchiveName", name.clone()))
        }
        "AWS::Events::Connection" => {
            let name = text(p, "Name").unwrap_or_else(|| super::generated_name(rc, 64, false));
            let mut body = pick(p, &["AuthorizationType", "AuthParameters", "Description"]);
            body.put("Name", name.clone().into());
            let made = call(rc, "CreateConnection", body).await?;
            let arn = made.at("ConnectionArn").clone();
            Ok(Made::new(&name).with("Arn", arn).with(
                "SecretArn",
                format!(
                    "arn:aws:secretsmanager:{}:{}:secret:events!connection/{name}",
                    rc.caller.region, rc.caller.account_id
                ),
            ))
        }
        "AWS::Events::ApiDestination" => {
            let name = text(p, "Name").unwrap_or_else(|| super::generated_name(rc, 64, false));
            let mut body = pick(
                p,
                &[
                    "ConnectionArn",
                    "InvocationEndpoint",
                    "HttpMethod",
                    "InvocationRateLimitPerSecond",
                    "Description",
                ],
            );
            body.put("Name", name.clone().into());
            let made = call(rc, "CreateApiDestination", body).await?;
            Ok(Made::new(&name).with("Arn", made.at("ApiDestinationArn").clone()))
        }
        other => Err(format!("{other} is not a type doze-aws makes")),
    }
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let id = old.physical.clone().unwrap_or_default();
    let before = old.props_value();
    match rc.ty.as_str() {
        "AWS::Events::Rule" => {
            let name = id.rsplit('|').next().unwrap_or_default().to_owned();
            put_rule(rc, &name, p).await?;
            retarget(rc, &name, &before, p).await?;
        }
        "AWS::Events::Connection" => {
            let mut body = pick(p, &["AuthorizationType", "AuthParameters", "Description"]);
            body.put("Name", id.clone().into());
            call(rc, "UpdateConnection", body).await?;
        }
        "AWS::Events::ApiDestination" => {
            let mut body = pick(
                p,
                &[
                    "ConnectionArn",
                    "InvocationEndpoint",
                    "HttpMethod",
                    "InvocationRateLimitPerSecond",
                    "Description",
                ],
            );
            body.put("Name", id.clone().into());
            call(rc, "UpdateApiDestination", body).await?;
        }
        "AWS::Events::Archive" => {
            let mut body = pick(p, &["Description", "RetentionDays"]);
            body.put("ArchiveName", id.clone().into());
            if let Some(pattern) = document(p, "EventPattern") {
                body.put("EventPattern", pattern.into());
            }
            call(rc, "UpdateArchive", body).await?;
        }
        _ => return create(rc, p).await,
    }
    let mut m = Made::new(id);
    m.attrs = old.attrs_map();
    Ok(m)
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let id = res.physical.clone().unwrap_or_default();
    let p = res.props_value();
    let ok = |r: Result<Value, aws_wire::ApiError>| match r {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    };
    match rc.ty.as_str() {
        "AWS::Events::Rule" => {
            let name = id.rsplit('|').next().unwrap_or_default().to_owned();
            retarget(
                rc,
                &name,
                &p,
                &json!({"EventBusName": text(&p, "EventBusName")}),
            )
            .await?;
            let mut body = json!({"Name": name});
            if let Some(bus) = bus_of(&p) {
                body.put("EventBusName", bus.into());
            }
            ok(rc.call(SVC, "DeleteRule", body).await)
        }
        "AWS::Events::EventBus" => ok(rc.call(SVC, "DeleteEventBus", json!({"Name": id})).await),
        "AWS::Events::EventBusPolicy" => {
            let body = json!({"EventBusName": text(&p, "EventBusName"), "StatementId": id});
            ok(rc.call(SVC, "RemovePermission", body).await)
        }
        "AWS::Events::Archive" => ok(rc
            .call(SVC, "DeleteArchive", json!({"ArchiveName": id}))
            .await),
        "AWS::Events::Connection" => {
            ok(rc.call(SVC, "DeleteConnection", json!({"Name": id})).await)
        }
        "AWS::Events::ApiDestination" => ok(rc
            .call(SVC, "DeleteApiDestination", json!({"Name": id}))
            .await),
        _ => Ok(()),
    }
}
