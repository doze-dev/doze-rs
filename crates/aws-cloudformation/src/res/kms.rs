//! `AWS::KMS::Key` and `AWS::KMS::Alias`. A key is deleted as CloudFormation deletes one: scheduled for deletion after
//! `PendingWindowInDays` (30 by default).

use serde_json::{Value, json};

use super::{Done, Made, Rc, document, gone, tag_list, text};
use crate::j::J;
use crate::model::Res;

const SVC: &str = "kms";

async fn call(rc: &Rc, op: &str, body: Value) -> Result<Value, String> {
    rc.call(SVC, op, body).await.map_err(|e| rc.fail(SVC, &e))
}

fn flag(p: &Value, key: &str, default: bool) -> bool {
    text(p, key).map_or(default, |v| v == "true")
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    if rc.ty == "AWS::KMS::Alias" {
        let name = text(p, "AliasName").unwrap_or_default();
        call(
            rc,
            "CreateAlias",
            json!({"AliasName": name, "TargetKeyId": text(p, "TargetKeyId")}),
        )
        .await?;
        return Ok(Made::new(name));
    }
    let mut body = json!({});
    for (from, to) in [
        ("Description", "Description"),
        ("KeySpec", "KeySpec"),
        ("KeyUsage", "KeyUsage"),
        ("Origin", "Origin"),
        ("MultiRegion", "MultiRegion"),
    ] {
        if let Some(v) = p.get(from).filter(|v| !v.is_null()) {
            body.put(to, v.clone());
        }
    }
    if let Some(policy) = document(p, "KeyPolicy") {
        body.put("Policy", policy.into());
    }
    let tags: Vec<Value> = tag_list(rc, p)
        .into_iter()
        .map(|(k, v)| json!({"TagKey": k, "TagValue": v}))
        .collect();
    if !tags.is_empty() {
        body.put("Tags", Value::Array(tags));
    }
    let made = call(rc, "CreateKey", body).await?;
    let meta = made.at("KeyMetadata");
    let id = meta.at("KeyId").as_str().unwrap_or_default().to_owned();
    settings(rc, &id, &Value::Null, p).await?;
    Ok(Made::new(&id)
        .with("KeyId", id.clone())
        .with("Arn", meta.at("Arn").clone()))
}

/// Rotation and the enabled state.
async fn settings(rc: &Rc, id: &str, before: &Value, p: &Value) -> Result<(), String> {
    let (rotate_then, rotate_now) = (
        flag(before, "EnableKeyRotation", false),
        flag(p, "EnableKeyRotation", false),
    );
    if rotate_then != rotate_now
        || text(before, "RotationPeriodInDays") != text(p, "RotationPeriodInDays")
    {
        if rotate_now {
            let mut body = json!({"KeyId": id});
            if let Some(d) = text(p, "RotationPeriodInDays") {
                body.put("RotationPeriodInDays", d.into());
            }
            call(rc, "EnableKeyRotation", body).await?;
        } else {
            call(rc, "DisableKeyRotation", json!({"KeyId": id})).await?;
        }
    }
    let (on_then, on_now) = (
        before.is_null() || flag(before, "Enabled", true),
        flag(p, "Enabled", true),
    );
    if on_then != on_now {
        call(
            rc,
            if on_now { "EnableKey" } else { "DisableKey" },
            json!({"KeyId": id}),
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let id = old.physical.clone().unwrap_or_default();
    let before = old.props_value();
    if rc.ty == "AWS::KMS::Alias" {
        call(
            rc,
            "UpdateAlias",
            json!({"AliasName": id, "TargetKeyId": text(p, "TargetKeyId")}),
        )
        .await?;
        return Ok(Made::new(id));
    }
    if text(&before, "Description") != text(p, "Description") {
        call(
            rc,
            "UpdateKeyDescription",
            json!({"KeyId": id, "Description": text(p, "Description").unwrap_or_default()}),
        )
        .await?;
    }
    if document(&before, "KeyPolicy") != document(p, "KeyPolicy")
        && let Some(policy) = document(p, "KeyPolicy")
    {
        call(
            rc,
            "PutKeyPolicy",
            json!({"KeyId": id, "PolicyName": "default", "Policy": policy}),
        )
        .await?;
    }
    settings(rc, &id, &before, p).await?;
    let mut m = Made::new(&id);
    m.attrs = old.attrs_map();
    Ok(m)
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let id = res.physical.clone().unwrap_or_default();
    let (op, body) = if rc.ty == "AWS::KMS::Alias" {
        ("DeleteAlias", json!({"AliasName": id}))
    } else {
        let days = text(&res.props_value(), "PendingWindowInDays").unwrap_or_else(|| "30".into());
        (
            "ScheduleKeyDeletion",
            json!({"KeyId": id, "PendingWindowInDays": days}),
        )
    };
    match rc.call(SVC, op, body).await {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) || e.message.contains("pending deletion") => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}
