//! `AWS::Kinesis::Stream` (shards, mode, retention, encryption, tags) and `AWS::Kinesis::StreamConsumer`.

use serde_json::{Map, Value, json};

use super::{Done, Made, Rc, gone, tag_list, text};
use crate::j::J;
use crate::model::Res;

const SVC: &str = "kinesis";

async fn call(rc: &Rc, op: &str, body: Value) -> Result<Value, String> {
    rc.call(SVC, op, body).await.map_err(|e| rc.fail(SVC, &e))
}

fn arn(rc: &Rc, name: &str) -> String {
    format!(
        "arn:aws:kinesis:{}:{}:stream/{name}",
        rc.caller.region, rc.caller.account_id
    )
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    if rc.ty == "AWS::Kinesis::StreamConsumer" {
        let body =
            json!({"StreamARN": text(p, "StreamARN"), "ConsumerName": text(p, "ConsumerName")});
        let made = call(rc, "RegisterStreamConsumer", body).await?;
        let c = made.at("Consumer");
        let consumer = c.at("ConsumerARN").as_str().unwrap_or_default().to_owned();
        return Ok(Made::new(&consumer)
            .with("ConsumerARN", consumer.clone())
            .with("ConsumerName", c.at("ConsumerName").clone())
            .with("ConsumerStatus", c.at("ConsumerStatus").clone())
            .with(
                "ConsumerCreationTimestamp",
                c.at("ConsumerCreationTimestamp").clone(),
            )
            .with("StreamARN", text(p, "StreamARN").unwrap_or_default()));
    }
    let name = text(p, "Name").unwrap_or_else(|| super::generated_name(rc, 128, false));
    if rc
        .call(SVC, "DescribeStreamSummary", json!({"StreamName": name}))
        .await
        .is_ok()
    {
        return Err(rc.taken(&name));
    }
    let on_demand = p.at("StreamModeDetails").at("StreamMode") == "ON_DEMAND";
    let mut body = json!({"StreamName": name});
    if let Some(mode) = p.get("StreamModeDetails") {
        body.put("StreamModeDetails", mode.clone());
    }
    if !on_demand {
        body.put(
            "ShardCount",
            p.get("ShardCount").cloned().unwrap_or(json!(1)),
        );
    }
    let tags: Map<String, Value> = tag_list(rc, p)
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    if !tags.is_empty() {
        body.put("Tags", Value::Object(tags));
    }
    call(rc, "CreateStream", body).await?;
    settings(rc, &name, &Value::Null, p).await?;
    Ok(Made::new(&name).with("Arn", arn(rc, &name)))
}

/// Retention and encryption, which have operations of their own.
async fn settings(rc: &Rc, name: &str, before: &Value, p: &Value) -> Result<(), String> {
    let hours = |v: &Value| {
        text(v, "RetentionPeriodHours")
            .and_then(|h| h.parse::<i64>().ok())
            .unwrap_or(24)
    };
    let (then, now) = (hours(before), hours(p));
    if then != now {
        let op = if now > then {
            "IncreaseStreamRetentionPeriod"
        } else {
            "DecreaseStreamRetentionPeriod"
        };
        call(
            rc,
            op,
            json!({"StreamName": name, "RetentionPeriodHours": now}),
        )
        .await?;
    }
    let (enc_then, enc_now) = (before.get("StreamEncryption"), p.get("StreamEncryption"));
    if enc_then != enc_now {
        match (enc_then, enc_now) {
            (_, Some(e)) => {
                let body = json!({"StreamName": name, "EncryptionType": e.at("EncryptionType"), "KeyId": e.at("KeyId")});
                call(rc, "StartStreamEncryption", body).await?;
            }
            (Some(e), None) => {
                let body = json!({"StreamName": name, "EncryptionType": e.at("EncryptionType"), "KeyId": e.at("KeyId")});
                call(rc, "StopStreamEncryption", body).await?;
            }
            (None, None) => {}
        }
    }
    Ok(())
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let name = old.physical.clone().unwrap_or_default();
    if rc.ty == "AWS::Kinesis::StreamConsumer" {
        let mut m = Made::new(name);
        m.attrs = old.attrs_map();
        return Ok(m);
    }
    let before = old.props_value();
    if text(&before, "ShardCount") != text(p, "ShardCount")
        && let Some(n) = text(p, "ShardCount")
    {
        call(
            rc,
            "UpdateShardCount",
            json!({"StreamName": name, "TargetShardCount": n, "ScalingType": "UNIFORM_SCALING"}),
        )
        .await?;
    }
    if before.get("StreamModeDetails") != p.get("StreamModeDetails")
        && let Some(mode) = p.get("StreamModeDetails")
    {
        call(
            rc,
            "UpdateStreamMode",
            json!({"StreamARN": arn(rc, &name), "StreamModeDetails": mode}),
        )
        .await?;
    }
    settings(rc, &name, &before, p).await?;
    let (then, now) = (tag_list(rc, &before), tag_list(rc, p));
    if then != now {
        let removed: Vec<&String> = then
            .iter()
            .map(|(k, _)| k)
            .filter(|k| !now.iter().any(|(n, _)| n == *k))
            .collect();
        if !removed.is_empty() {
            call(
                rc,
                "RemoveTagsFromStream",
                json!({"StreamName": name, "TagKeys": removed}),
            )
            .await?;
        }
        let tags: Map<String, Value> = now
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect();
        if !tags.is_empty() {
            call(
                rc,
                "AddTagsToStream",
                json!({"StreamName": name, "Tags": tags}),
            )
            .await?;
        }
    }
    Ok(Made::new(&name).with("Arn", arn(rc, &name)))
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let id = res.physical.clone().unwrap_or_default();
    let (op, body) = if rc.ty == "AWS::Kinesis::StreamConsumer" {
        ("DeregisterStreamConsumer", json!({"ConsumerARN": id}))
    } else {
        (
            "DeleteStream",
            json!({"StreamName": id, "EnforceConsumerDeletion": true}),
        )
    };
    match rc.call(SVC, op, body).await {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}
