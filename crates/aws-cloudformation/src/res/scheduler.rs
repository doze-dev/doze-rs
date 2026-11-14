//! `AWS::Scheduler::Schedule` and `AWS::Scheduler::ScheduleGroup`, through EventBridge Scheduler's own operations. `Ref` is the name
//! of either (registry schema, memory); `Fn::GetAtt` gives `Arn`, and a group's `State`, `CreationDate` and `LastModificationDate`.

use serde_json::{Value, json};

use super::{Done, Made, Rc, gone, tag_list, text};
use crate::j::J;
use crate::model::Res;

const SVC: &str = "scheduler";

/// The schedule members a template passes through as they are.
const SCHEDULE: &[&str] = &[
    "GroupName",
    "ScheduleExpression",
    "ScheduleExpressionTimezone",
    "Description",
    "State",
    "KmsKeyArn",
    "FlexibleTimeWindow",
    "Target",
    "ActionAfterCompletion",
];

async fn call(rc: &Rc, op: &str, body: Value) -> Result<Value, String> {
    rc.call(SVC, op, body).await.map_err(|e| rc.fail(SVC, &e))
}

/// The request `CreateSchedule` and `UpdateSchedule` share. A template gives `StartDate` and `EndDate` as ISO 8601 text; the API
/// takes seconds.
fn schedule_body(name: &str, p: &Value) -> Value {
    let mut body = json!({"Name": name});
    for k in SCHEDULE {
        if let Some(v) = p.get(*k).filter(|v| !v.is_null()) {
            body.put(k, v.clone());
        }
    }
    for k in ["StartDate", "EndDate"] {
        if let Some(secs) = text(p, k).and_then(|t| aws_core::time::parse_rfc3339(&t)) {
            body.put(k, json!(secs));
        }
    }
    body
}

fn group_of(p: &Value) -> String {
    text(p, "GroupName").unwrap_or_else(|| "default".to_owned())
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    if rc.ty == "AWS::Scheduler::ScheduleGroup" {
        let name = text(p, "Name").unwrap_or_else(|| super::generated_name(rc, 64, false));
        let tags: Vec<Value> = tag_list(rc, p)
            .into_iter()
            .map(|(k, v)| json!({"Key": k, "Value": v}))
            .collect();
        let mut body = json!({"Name": name});
        if !tags.is_empty() {
            body.put("Tags", Value::Array(tags));
        }
        call(rc, "CreateScheduleGroup", body).await?;
        return group_made(rc, &name).await;
    }
    let name = text(p, "Name").unwrap_or_else(|| super::generated_name(rc, 64, false));
    let made = call(rc, "CreateSchedule", schedule_body(&name, p)).await?;
    Ok(Made::new(&name).with("Arn", made.at("ScheduleArn").clone()))
}

async fn group_made(rc: &Rc, name: &str) -> Done {
    let g = call(rc, "GetScheduleGroup", json!({"Name": name})).await?;
    let mut m = Made::new(name);
    for k in ["Arn", "State", "CreationDate", "LastModificationDate"] {
        m = m.with(k, g.at(k).clone());
    }
    Ok(m)
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let name = old.physical.clone().unwrap_or_default();
    if rc.ty == "AWS::Scheduler::ScheduleGroup" {
        let arn = old.attrs_map().get("Arn").cloned().unwrap_or(Value::Null);
        let before = old.props_value();
        let (then, now) = (tag_list(rc, &before), tag_list(rc, p));
        let gone_keys: Vec<&String> = then
            .iter()
            .map(|(k, _)| k)
            .filter(|k| !now.iter().any(|(n, _)| n == *k))
            .collect();
        if !gone_keys.is_empty() {
            call(
                rc,
                "UntagResource",
                json!({"ResourceArn": arn, "TagKeys": gone_keys}),
            )
            .await?;
        }
        if !now.is_empty() {
            let tags: Vec<Value> = now
                .iter()
                .map(|(k, v)| json!({"Key": k, "Value": v}))
                .collect();
            call(rc, "TagResource", json!({"ResourceArn": arn, "Tags": tags})).await?;
        }
        return group_made(rc, &name).await;
    }
    let made = call(rc, "UpdateSchedule", schedule_body(&name, p)).await?;
    Ok(Made::new(&name).with("Arn", made.at("ScheduleArn").clone()))
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let name = res.physical.clone().unwrap_or_default();
    let (op, body) = if rc.ty == "AWS::Scheduler::ScheduleGroup" {
        ("DeleteScheduleGroup", json!({"Name": name}))
    } else {
        (
            "DeleteSchedule",
            json!({"Name": name, "GroupName": group_of(&res.props_value())}),
        )
    };
    match rc.call(SVC, op, body).await {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}
