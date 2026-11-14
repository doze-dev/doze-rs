//! `AWS::SNS::Topic` (with inline subscriptions), `AWS::SNS::Subscription`, `AWS::SNS::TopicPolicy` and
//! `AWS::SNS::TopicInlinePolicy`.

use serde_json::{Map, Value, json};

use super::{Done, Made, Rc, document, gone, tag_list, text};
use crate::j::J;
use crate::model::Res;

const SVC: &str = "sns";

const TOPIC_ATTRIBUTES: &[&str] = &[
    "DisplayName",
    "KmsMasterKeyId",
    "ContentBasedDeduplication",
    "ArchivePolicy",
    "SignatureVersion",
    "TracingConfig",
    "FifoThroughputScope",
];

const SUBSCRIPTION_ATTRIBUTES: &[&str] = &[
    "FilterPolicy",
    "FilterPolicyScope",
    "RawMessageDelivery",
    "RedrivePolicy",
    "DeliveryPolicy",
    "SubscriptionRoleArn",
    "ReplayPolicy",
];

fn picked(p: &Value, keys: &[&str]) -> Map<String, Value> {
    keys.iter()
        .filter_map(|k| document(p, k).map(|v| ((*k).to_owned(), Value::String(v))))
        .collect()
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    match rc.ty.as_str() {
        "AWS::SNS::Topic" => topic(rc, p).await,
        "AWS::SNS::Subscription" => {
            let mut body = json!({"TopicArn": text(p, "TopicArn"), "Protocol": text(p, "Protocol"),
                "Attributes": picked(p, SUBSCRIPTION_ATTRIBUTES), "ReturnSubscriptionArn": true});
            if let Some(e) = text(p, "Endpoint") {
                body.put("Endpoint", e.into());
            }
            let made = rc
                .call(SVC, "Subscribe", body)
                .await
                .map_err(|e| rc.fail(SVC, &e))?;
            Ok(Made::new(
                made.at("SubscriptionArn").as_str().unwrap_or_default(),
            ))
        }
        "AWS::SNS::TopicPolicy" => {
            for arn in topics(p) {
                set_policy(rc, &arn, p).await?;
            }
            Ok(Made::new(super::generated_name(rc, 128, false)))
        }
        _ => {
            let arn = text(p, "TopicArn").unwrap_or_default();
            set_policy(rc, &arn, p).await?;
            Ok(Made::new(arn))
        }
    }
}

async fn topic(rc: &Rc, p: &Value) -> Done {
    let fifo = text(p, "FifoTopic").as_deref() == Some("true");
    let name = text(p, "TopicName").unwrap_or_else(|| {
        if fifo {
            super::naming::generated_with(rc, 256, false, ".fifo")
        } else {
            super::generated_name(rc, 256, false)
        }
    });
    let arn = format!(
        "arn:aws:sns:{}:{}:{name}",
        rc.caller.region, rc.caller.account_id
    );
    if rc
        .call(SVC, "GetTopicAttributes", json!({"TopicArn": arn}))
        .await
        .is_ok()
    {
        return Err(rc.taken(&arn));
    }
    let mut attrs = picked(p, TOPIC_ATTRIBUTES);
    if fifo {
        attrs.insert("FifoTopic".into(), "true".into());
    }
    let tags: Vec<Value> = tag_list(rc, p)
        .into_iter()
        .map(|(k, v)| json!({"Key": k, "Value": v}))
        .collect();
    let mut body = json!({"Name": name, "Attributes": attrs});
    if !tags.is_empty() {
        body.put("Tags", Value::Array(tags));
    }
    rc.call(SVC, "CreateTopic", body)
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    if let Some(dp) = document(p, "DataProtectionPolicy") {
        rc.call(
            SVC,
            "PutDataProtectionPolicy",
            json!({"ResourceArn": arn, "DataProtectionPolicy": dp}),
        )
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    }
    for s in subscriptions(p) {
        rc.call(
            SVC,
            "Subscribe",
            json!({"TopicArn": arn, "Protocol": s.0, "Endpoint": s.1}),
        )
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    }
    Ok(Made::new(&arn)
        .with("TopicArn", arn.clone())
        .with("TopicName", name))
}

/// The inline subscriptions of a topic: protocol and endpoint.
fn subscriptions(p: &Value) -> Vec<(String, String)> {
    p.get("Subscription")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|s| {
                    (
                        text(s, "Protocol").unwrap_or_default(),
                        text(s, "Endpoint").unwrap_or_default(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn topics(p: &Value) -> Vec<String> {
    p.get("Topics")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

async fn set_policy(rc: &Rc, arn: &str, p: &Value) -> Result<(), String> {
    let policy = document(p, "PolicyDocument").unwrap_or_default();
    rc.call(
        SVC,
        "SetTopicAttributes",
        json!({"TopicArn": arn, "AttributeName": "Policy", "AttributeValue": policy}),
    )
    .await
    .map(|_| ())
    .map_err(|e| rc.fail(SVC, &e))
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let physical = old.physical.clone().unwrap_or_default();
    let before = old.props_value();
    match rc.ty.as_str() {
        "AWS::SNS::Topic" => {
            let then = picked(&before, TOPIC_ATTRIBUTES);
            for (k, v) in picked(p, TOPIC_ATTRIBUTES) {
                if then.get(&k) != Some(&v) {
                    rc.call(
                        SVC,
                        "SetTopicAttributes",
                        json!({"TopicArn": physical, "AttributeName": k, "AttributeValue": v}),
                    )
                    .await
                    .map_err(|e| rc.fail(SVC, &e))?;
                }
            }
            resubscribe(rc, &physical, &before, p).await?;
            let name = physical.rsplit(':').next().unwrap_or_default().to_owned();
            Ok(Made::new(&physical)
                .with("TopicArn", physical.clone())
                .with("TopicName", name))
        }
        "AWS::SNS::Subscription" => {
            let then = picked(&before, SUBSCRIPTION_ATTRIBUTES);
            let now = picked(p, SUBSCRIPTION_ATTRIBUTES);
            for k in SUBSCRIPTION_ATTRIBUTES {
                let (a, b) = (then.get(*k), now.get(*k));
                if a != b {
                    let value = b.cloned().unwrap_or_else(|| Value::String(String::new()));
                    rc.call(SVC, "SetSubscriptionAttributes", json!({"SubscriptionArn": physical, "AttributeName": k, "AttributeValue": value}))
                        .await
                        .map_err(|e| rc.fail(SVC, &e))?;
                }
            }
            Ok(Made::new(physical))
        }
        _ => {
            for arn in if rc.ty == "AWS::SNS::TopicPolicy" {
                topics(p)
            } else {
                text(p, "TopicArn").into_iter().collect()
            } {
                set_policy(rc, &arn, p).await?;
            }
            Ok(Made::new(physical))
        }
    }
}

/// Subscribes what the template now lists and unsubscribes what it no longer lists.
async fn resubscribe(rc: &Rc, arn: &str, before: &Value, p: &Value) -> Result<(), String> {
    let (then, now) = (subscriptions(before), subscriptions(p));
    if then == now {
        return Ok(());
    }
    let listed = rc
        .call(SVC, "ListSubscriptionsByTopic", json!({"TopicArn": arn}))
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    for s in listed.at("Subscriptions").as_array().into_iter().flatten() {
        let key = (
            text(s, "Protocol").unwrap_or_default(),
            text(s, "Endpoint").unwrap_or_default(),
        );
        if then.contains(&key)
            && !now.contains(&key)
            && let Some(sub) = s
                .at("SubscriptionArn")
                .as_str()
                .filter(|a| a.starts_with("arn:"))
        {
            drop(
                rc.call(SVC, "Unsubscribe", json!({"SubscriptionArn": sub}))
                    .await,
            );
        }
    }
    for (proto, endpoint) in now.iter().filter(|s| !then.contains(s)) {
        rc.call(
            SVC,
            "Subscribe",
            json!({"TopicArn": arn, "Protocol": proto, "Endpoint": endpoint}),
        )
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    }
    Ok(())
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let physical = res.physical.clone().unwrap_or_default();
    let (op, body) = match rc.ty.as_str() {
        "AWS::SNS::Topic" => ("DeleteTopic", json!({"TopicArn": physical})),
        "AWS::SNS::Subscription" if physical.starts_with("arn:") => {
            ("Unsubscribe", json!({"SubscriptionArn": physical}))
        }
        // A policy stays on a topic until the topic goes: SNS has no call that puts the default back.
        _ => return Ok(()),
    };
    match rc.call(SVC, op, body).await {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}
