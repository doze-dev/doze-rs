//! `AWS::DynamoDB::Table` and `AWS::DynamoDB::GlobalTable` (its replica in this region; replicas in other regions are not made, and
//! the status reason says so).

use serde_json::{Map, Value, json};

use super::{Done, Made, Rc, gone, tag_list, text};
use crate::j::J;
use crate::model::Res;

const SVC: &str = "dynamodb";

fn table_body(name: &str, p: &Value) -> Value {
    let mut b = json!({"TableName": name});
    for k in [
        "AttributeDefinitions",
        "KeySchema",
        "BillingMode",
        "ProvisionedThroughput",
        "GlobalSecondaryIndexes",
        "LocalSecondaryIndexes",
        "TableClass",
        "DeletionProtectionEnabled",
        "OnDemandThroughput",
        "WarmThroughput",
    ] {
        if let Some(v) = p.get(k).filter(|v| !v.is_null()) {
            b.put(k, v.clone());
        }
    }
    if let Some(view) = p
        .get("StreamSpecification")
        .and_then(|s| s.get("StreamViewType"))
    {
        b.put(
            "StreamSpecification",
            json!({"StreamEnabled": true, "StreamViewType": view}),
        );
    }
    if let Some(sse) = p.get("SSESpecification").filter(|v| !v.is_null()) {
        b.put("SSESpecification", sse.clone());
    }
    // A global table's write capacity sits on its own property.
    if let Some(w) = p.get("WriteProvisionedThroughputSettings") {
        let units = w
            .get("WriteCapacityAutoScalingSettings")
            .and_then(|a| a.get("MinCapacity"))
            .cloned()
            .unwrap_or(json!(5));
        b.put(
            "ProvisionedThroughput",
            json!({"ReadCapacityUnits": units, "WriteCapacityUnits": units}),
        );
    }
    b
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    let p = &local_replica(rc, p);
    let name = text(p, "TableName").unwrap_or_else(|| super::generated_name(rc, 255, false));
    if rc
        .call(SVC, "DescribeTable", json!({"TableName": name}))
        .await
        .is_ok()
    {
        return Err(rc.taken(&name));
    }
    let mut body = table_body(&name, p);
    let tags: Vec<Value> = tag_list(rc, p)
        .into_iter()
        .map(|(k, v)| json!({"Key": k, "Value": v}))
        .collect();
    if !tags.is_empty() {
        body.put("Tags", Value::Array(tags));
    }
    let made = rc
        .call(SVC, "CreateTable", body)
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    let desc = &made.at("TableDescription");
    side_settings(rc, &name, &Value::Null, p).await?;
    Ok(described(desc))
}

/// A global table's properties with its replica in this region folded in (tags, PITR, the rest are per replica).
fn local_replica(rc: &Rc, p: &Value) -> Value {
    if rc.ty != "AWS::DynamoDB::GlobalTable" {
        return p.clone();
    }
    let mut out = p.clone();
    if let Some(r) = p.get("Replicas").and_then(Value::as_array).and_then(|a| {
        a.iter()
            .find(|r| text(r, "Region").as_deref() == Some(rc.caller.region.as_str()))
    }) {
        for k in [
            "Tags",
            "PointInTimeRecoverySpecification",
            "TableClass",
            "DeletionProtectionEnabled",
        ] {
            if let Some(v) = r.get(k) {
                out.put(k, v.clone());
            }
        }
    }
    if let Some(ttl) = p.get("TimeToLiveSpecification") {
        out.put("TimeToLiveSpecification", ttl.clone());
    }
    out
}

fn described(desc: &Value) -> Made {
    let mut m = Made::new(desc.at("TableName").as_str().unwrap_or_default())
        .with("Arn", desc.at("TableArn").clone())
        .with("TableId", desc.at("TableId").clone());
    if let Some(s) = desc.at("LatestStreamArn").as_str() {
        m = m.with("StreamArn", s);
    }
    m
}

/// Time to live and point-in-time recovery, which are set with operations of their own.
async fn side_settings(rc: &Rc, name: &str, before: &Value, p: &Value) -> Result<(), String> {
    let ttl = p.get("TimeToLiveSpecification");
    if ttl != before.get("TimeToLiveSpecification")
        && let Some(t) = ttl.or_else(|| before.get("TimeToLiveSpecification"))
    {
        let enabled = ttl.is_some_and(|t| text(t, "Enabled").as_deref() == Some("true"));
        rc.call(SVC, "UpdateTimeToLive", json!({"TableName": name,
            "TimeToLiveSpecification": {"AttributeName": t.get("AttributeName"), "Enabled": enabled}}))
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
    }
    let pitr = p.get("PointInTimeRecoverySpecification");
    if pitr != before.get("PointInTimeRecoverySpecification") {
        let enabled =
            pitr.is_some_and(|x| text(x, "PointInTimeRecoveryEnabled").as_deref() == Some("true"));
        rc.call(
            SVC,
            "UpdateContinuousBackups",
            json!({"TableName": name,
            "PointInTimeRecoverySpecification": {"PointInTimeRecoveryEnabled": enabled}}),
        )
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    }
    Ok(())
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let p = &local_replica(rc, p);
    let name = old.physical.clone().unwrap_or_default();
    let before = local_replica(rc, &old.props_value());
    let mut body = json!({"TableName": name});
    let mut changed = false;
    for k in [
        "BillingMode",
        "ProvisionedThroughput",
        "TableClass",
        "DeletionProtectionEnabled",
        "SSESpecification",
        "OnDemandThroughput",
    ] {
        if p.get(k) != before.get(k)
            && let Some(v) = p.get(k)
        {
            body.put(k, v.clone());
            changed = true;
        }
    }
    if p.get("StreamSpecification") != before.get("StreamSpecification") {
        body.put(
            "StreamSpecification",
            match p
                .get("StreamSpecification")
                .and_then(|s| s.get("StreamViewType"))
            {
                Some(view) => json!({"StreamEnabled": true, "StreamViewType": view}),
                None => json!({"StreamEnabled": false}),
            },
        );
        changed = true;
    }
    if changed {
        rc.call(SVC, "UpdateTable", body)
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
    }
    indexes(rc, &name, &before, p).await?;
    side_settings(rc, &name, &before, p).await?;
    let desc = rc
        .call(SVC, "DescribeTable", json!({"TableName": name}))
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    retag(
        rc,
        desc.at("Table").at("TableArn").as_str().unwrap_or_default(),
        &tag_list(rc, &before),
        &tag_list(rc, p),
    )
    .await?;
    Ok(described(desc.at("Table")))
}

/// Global secondary indexes added and removed, one per call as DynamoDB allows.
async fn indexes(rc: &Rc, name: &str, before: &Value, p: &Value) -> Result<(), String> {
    let list = |v: &Value| -> Map<String, Value> {
        v.get("GlobalSecondaryIndexes")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|i| (text(i, "IndexName").unwrap_or_default(), i.clone()))
                    .collect()
            })
            .unwrap_or_default()
    };
    let (then, now) = (list(before), list(p));
    for gone_name in then.keys().filter(|k| !now.contains_key(*k)) {
        rc.call(SVC, "UpdateTable", json!({"TableName": name, "GlobalSecondaryIndexUpdates": [{"Delete": {"IndexName": gone_name}}]}))
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
    }
    for (index, def) in &now {
        if then.get(index) == Some(def) {
            continue;
        }
        let update = if then.contains_key(index) {
            json!({"Update": {"IndexName": index, "ProvisionedThroughput": def.get("ProvisionedThroughput")}})
        } else {
            json!({"Create": def})
        };
        let mut body = json!({"TableName": name, "GlobalSecondaryIndexUpdates": [update]});
        if !then.contains_key(index) {
            body.put(
                "AttributeDefinitions",
                p.get("AttributeDefinitions").cloned().unwrap_or(json!([])),
            );
        }
        rc.call(SVC, "UpdateTable", body)
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
    }
    Ok(())
}

async fn retag(
    rc: &Rc,
    arn: &str,
    then: &[(String, String)],
    now: &[(String, String)],
) -> Result<(), String> {
    if then == now {
        return Ok(());
    }
    let removed: Vec<&String> = then
        .iter()
        .map(|(k, _)| k)
        .filter(|k| !now.iter().any(|(n, _)| n == *k))
        .collect();
    if !removed.is_empty() {
        rc.call(
            SVC,
            "UntagResource",
            json!({"ResourceArn": arn, "TagKeys": removed}),
        )
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    }
    if !now.is_empty() {
        let tags: Vec<Value> = now
            .iter()
            .map(|(k, v)| json!({"Key": k, "Value": v}))
            .collect();
        rc.call(
            SVC,
            "TagResource",
            json!({"ResourceArn": arn, "Tags": tags}),
        )
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    }
    Ok(())
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let name = res.physical.clone().unwrap_or_default();
    match rc
        .call(SVC, "DeleteTable", json!({"TableName": name}))
        .await
    {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) || e.code.contains("ResourceNotFound") => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}
