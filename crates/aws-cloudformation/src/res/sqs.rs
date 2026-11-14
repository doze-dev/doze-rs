//! `AWS::SQS::Queue`, `AWS::SQS::QueuePolicy` and `AWS::SQS::QueueInlinePolicy`.

use serde_json::{Map, Value, json};

use super::{Done, Made, Rc, document, gone, tag_list, text};
use crate::j::J;
use crate::model::Res;

const SVC: &str = "sqs";

/// The queue attributes a template sets, with what removing one goes back to (SQS's defaults).
const ATTRIBUTES: &[(&str, Option<&str>)] = &[
    ("ContentBasedDeduplication", None),
    ("DeduplicationScope", None),
    ("DelaySeconds", Some("0")),
    ("FifoThroughputLimit", None),
    ("KmsDataKeyReusePeriodSeconds", Some("300")),
    ("KmsMasterKeyId", Some("")),
    ("MaximumMessageSize", Some("1048576")),
    ("MessageRetentionPeriod", Some("345600")),
    ("ReceiveMessageWaitTimeSeconds", Some("0")),
    ("RedriveAllowPolicy", Some("")),
    ("RedrivePolicy", Some("")),
    ("SqsManagedSseEnabled", None),
    ("VisibilityTimeout", Some("30")),
];

fn attributes(p: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    for (k, _) in ATTRIBUTES {
        if let Some(v) = document(p, k) {
            out.insert((*k).to_owned(), Value::String(v));
        }
    }
    if text(p, "FifoQueue").as_deref() == Some("true") {
        out.insert("FifoQueue".into(), "true".into());
    }
    out
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    match rc.ty.as_str() {
        "AWS::SQS::Queue" => queue(rc, p).await,
        "AWS::SQS::QueuePolicy" => {
            for url in urls(p) {
                set_policy(rc, &url, &document(p, "PolicyDocument").unwrap_or_default()).await?;
            }
            Ok(Made::new(super::generated_name(rc, 128, false)))
        }
        "AWS::SQS::QueueInlinePolicy" => {
            let url = text(p, "Queue").unwrap_or_default();
            set_policy(rc, &url, &document(p, "PolicyDocument").unwrap_or_default()).await?;
            Ok(Made::new(url))
        }
        other => Err(format!("{other} is not a type doze-aws makes")),
    }
}

async fn queue(rc: &Rc, p: &Value) -> Done {
    let fifo = text(p, "FifoQueue").as_deref() == Some("true");
    let name = text(p, "QueueName").unwrap_or_else(|| {
        if fifo {
            super::naming::generated_with(rc, 80, false, ".fifo")
        } else {
            super::generated_name(rc, 80, false)
        }
    });
    if rc
        .call(SVC, "GetQueueUrl", json!({"QueueName": name}))
        .await
        .is_ok()
    {
        // Recorded 2026-11-12: a stack never takes over a queue it did not make.
        return Err(rc.handler(
            "Queue creation failed because the queue already exists",
            "AlreadyExists",
        ));
    }
    let tags: Map<String, Value> = tag_list(rc, p)
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    let mut body = json!({"QueueName": name, "Attributes": attributes(p)});
    if !tags.is_empty() {
        body.put("tags", Value::Object(tags));
    }
    let made = rc
        .call(SVC, "CreateQueue", body)
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    let url = made.at("QueueUrl").as_str().unwrap_or_default().to_owned();
    Ok(described(rc, &url, &name))
}

fn described(rc: &Rc, url: &str, name: &str) -> Made {
    let arn = format!(
        "arn:aws:sqs:{}:{}:{name}",
        rc.caller.region, rc.caller.account_id
    );
    Made::new(url)
        .with("Arn", arn)
        .with("QueueName", name)
        .with("QueueUrl", url)
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    if rc.ty != "AWS::SQS::Queue" {
        // A policy is put again on the queues it names, and taken off those it no longer names.
        let before = old.props_value();
        let now: Vec<String> = if rc.ty == "AWS::SQS::QueuePolicy" {
            urls(p)
        } else {
            text(p, "Queue").into_iter().collect()
        };
        let then: Vec<String> = if rc.ty == "AWS::SQS::QueuePolicy" {
            urls(&before)
        } else {
            text(&before, "Queue").into_iter().collect()
        };
        for url in then.iter().filter(|u| !now.contains(u)) {
            drop(set_policy(rc, url, "").await);
        }
        for url in &now {
            set_policy(rc, url, &document(p, "PolicyDocument").unwrap_or_default()).await?;
        }
        return Ok(Made::new(old.physical.clone().unwrap_or_default()));
    }
    let url = old.physical.clone().unwrap_or_default();
    let before = old.props_value();
    let mut attrs = attributes(p);
    for (k, default) in ATTRIBUTES {
        if before.get(*k).is_some()
            && p.get(*k).is_none()
            && let Some(d) = default
        {
            attrs.insert((*k).to_owned(), Value::String((*d).to_owned()));
        }
    }
    attrs.remove("FifoQueue");
    if !attrs.is_empty() {
        rc.call(
            SVC,
            "SetQueueAttributes",
            json!({"QueueUrl": url, "Attributes": attrs}),
        )
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    }
    retag(rc, &url, &before, p).await?;
    let name = url.rsplit('/').next().unwrap_or_default().to_owned();
    Ok(described(rc, &url, &name))
}

async fn retag(rc: &Rc, url: &str, before: &Value, p: &Value) -> Result<(), String> {
    let old = tag_list(rc, before);
    let new = tag_list(rc, p);
    let gone_keys: Vec<&String> = old
        .iter()
        .map(|(k, _)| k)
        .filter(|k| !new.iter().any(|(n, _)| n == *k))
        .collect();
    if !gone_keys.is_empty() {
        rc.call(
            SVC,
            "UntagQueue",
            json!({"QueueUrl": url, "TagKeys": gone_keys}),
        )
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    }
    if new != old && !new.is_empty() {
        let tags: Map<String, Value> = new
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect();
        rc.call(SVC, "TagQueue", json!({"QueueUrl": url, "Tags": tags}))
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
    }
    Ok(())
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let p = res.props_value();
    match rc.ty.as_str() {
        "AWS::SQS::Queue" => {
            let url = res.physical.clone().unwrap_or_default();
            match rc.call(SVC, "DeleteQueue", json!({"QueueUrl": url})).await {
                Ok(_) => Ok(()),
                Err(e) if gone(&e) || e.code.contains("NonExistentQueue") => Ok(()),
                Err(e) => Err(rc.fail(SVC, &e)),
            }
        }
        "AWS::SQS::QueuePolicy" => {
            for url in urls(&p) {
                drop(set_policy(rc, &url, "").await);
            }
            Ok(())
        }
        _ => {
            drop(set_policy(rc, &text(&p, "Queue").unwrap_or_default(), "").await);
            Ok(())
        }
    }
}

fn urls(p: &Value) -> Vec<String> {
    p.get("Queues")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

async fn set_policy(rc: &Rc, url: &str, policy: &str) -> Result<(), String> {
    rc.call(
        SVC,
        "SetQueueAttributes",
        json!({"QueueUrl": url, "Attributes": {"Policy": policy}}),
    )
    .await
    .map(|_| ())
    .map_err(|e| rc.fail(SVC, &e))
}
