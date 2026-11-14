//! The declared resources as a CloudFormation template: the module keeps them as one stack on the instance's own
//! CloudFormation (`converge.rs`), so what doze.hcl adds is created and what it changes is updated in place, by the same
//! resource handlers `doze-aws apply` uses (Go's module only created what was missing). What it no longer declares
//! leaves the stack but is kept, data and all ([`resource`]).
//!
//! Each block becomes one resource (a queue with `dlq = "auto"` two, a function with triggers or destinations a few),
//! named by its kind and label. References between blocks (`queue = "emails"`) are references in the template, so
//! CloudFormation orders the work; `check.rs` has already refused a reference to something not declared.

use serde_json::{Map, Value, json};

use super::spec::{AwsSpec, Bucket, Index, Queue, Table, Topic};
use super::template_more::{function, key, parameter, rule, secret, state_machine};

/// The stack's resources, by logical id.
pub(crate) type Resources = Map<String, Value>;

/// A logical id for a block: its kind and its label's letters and digits, then a hash of the label when anything
/// was dropped or changed, so `a-b` and `ab` never meet. Stable across runs: the stack's resources keep their ids.
pub(crate) fn id(kind: &str, name: &str) -> String {
    let clean: String = name.chars().filter(char::is_ascii_alphanumeric).collect();
    if clean == name && !clean.is_empty() {
        return format!("{kind}{clean}");
    }
    // FNV-1a: stable everywhere, no dependency.
    let hash = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{kind}{clean}{:08x}", hash & 0xffff_ffff)
}

pub(crate) fn arn(kind: &str, name: &str) -> Value {
    json!({ "Fn::GetAtt": [id(kind, name), "Arn"] })
}

pub(crate) fn tags(t: &std::collections::BTreeMap<String, String>) -> Option<Value> {
    (!t.is_empty()).then(|| {
        Value::Array(
            t.iter()
                .map(|(k, v)| json!({ "Key": k, "Value": v }))
                .collect(),
        )
    })
}

/// Sets `key` when there is a value.
pub(crate) fn put(props: &mut Map<String, Value>, key: &str, v: Option<Value>) {
    if let Some(v) = v {
        props.insert(key.into(), v);
    }
}

/// A resource, retained when it leaves the stack: a block taken out of doze.hcl leaves its bucket, queue or table and
/// their data where they are (as Go's module, which never deleted), and doze stops keeping it.
pub(crate) fn resource(kind: &str, props: Map<String, Value>) -> Value {
    json!({
        "Type": kind,
        "Properties": props,
        "DeletionPolicy": "Retain",
        "UpdateReplacePolicy": "Retain",
    })
}

/// The whole template.
pub(crate) fn template(s: &AwsSpec, dir: &str) -> Value {
    let mut r = Resources::new();
    for b in &s.bucket {
        bucket(&mut r, b);
    }
    for q in &s.queue {
        queue(&mut r, q);
    }
    for t in &s.topic {
        topic(&mut r, t);
    }
    for t in &s.table {
        table(&mut r, t);
    }
    for f in &s.function {
        function(&mut r, f, dir);
    }
    for x in &s.rule {
        rule(&mut r, x);
    }
    for k in &s.key {
        key(&mut r, k);
    }
    for x in &s.secret {
        secret(&mut r, x);
    }
    for p in &s.parameter {
        parameter(&mut r, p);
    }
    for m in &s.state_machine {
        state_machine(&mut r, m);
    }
    // A stack holds at least one resource: this no-op one, so a block with nothing declared any more is still a stack.
    r.insert(
        "DozeKept".into(),
        json!({ "Type": "AWS::CloudFormation::WaitConditionHandle", "Properties": {} }),
    );
    json!({
        "AWSTemplateFormatVersion": "2010-09-09",
        "Description": "Kept by doze from doze.hcl: edit the aws block, not this stack.",
        "Resources": r,
    })
}

/// A queue's name as SQS has it: a FIFO queue's ends in `.fifo`.
pub(crate) fn queue_name(q: &Queue) -> String {
    if q.fifo && !q.name.ends_with(".fifo") {
        format!("{}.fifo", q.name)
    } else {
        q.name.clone()
    }
}

fn bucket(r: &mut Resources, b: &Bucket) {
    let mut p = Map::new();
    p.insert("BucketName".into(), b.name.clone().into());
    if b.versioning || b.object_lock {
        p.insert(
            "VersioningConfiguration".into(),
            json!({ "Status": "Enabled" }),
        );
    }
    if b.object_lock {
        p.insert("ObjectLockEnabled".into(), true.into());
    }
    put(&mut p, "Tags", tags(&b.tags));
    let (mut queues, mut topics, mut lambdas) = (Vec::new(), Vec::new(), Vec::new());
    for n in &b.notify {
        let events = if n.events.is_empty() {
            vec!["s3:ObjectCreated:*".to_owned()]
        } else {
            n.events.clone()
        };
        let rules: Vec<Value> = [("prefix", &n.prefix), ("suffix", &n.suffix)]
            .into_iter()
            .filter_map(|(k, v)| v.as_ref().map(|v| json!({ "Name": k, "Value": v })))
            .collect();
        for e in events {
            let mut c = Map::new();
            c.insert("Event".into(), e.into());
            if !rules.is_empty() {
                c.insert("Filter".into(), json!({ "S3Key": { "Rules": rules } }));
            }
            if let Some(q) = &n.queue {
                c.insert("Queue".into(), arn("Queue", q));
                queues.push(Value::Object(c));
            } else if let Some(t) = &n.topic {
                c.insert("Topic".into(), json!({ "Ref": id("Topic", t) }));
                topics.push(Value::Object(c));
            } else if let Some(f) = &n.lambda {
                c.insert("Function".into(), arn("Function", f));
                lambdas.push(Value::Object(c));
            }
        }
    }
    if !(queues.is_empty() && topics.is_empty() && lambdas.is_empty()) {
        p.insert(
            "NotificationConfiguration".into(),
            json!({
                "QueueConfigurations": queues,
                "TopicConfigurations": topics,
                "LambdaConfigurations": lambdas,
            }),
        );
    }
    r.insert(id("Bucket", &b.name), resource("AWS::S3::Bucket", p));
}

fn queue(r: &mut Resources, q: &Queue) {
    let mut p = Map::new();
    p.insert("QueueName".into(), queue_name(q).into());
    if q.fifo {
        p.insert("FifoQueue".into(), true.into());
    }
    if q.content_dedup {
        p.insert("ContentBasedDeduplication".into(), true.into());
    }
    for (k, v) in [
        ("VisibilityTimeout", q.visibility),
        ("DelaySeconds", q.delay),
        ("MessageRetentionPeriod", q.retention),
        ("ReceiveMessageWaitTimeSeconds", q.receive_wait),
        ("MaximumMessageSize", q.max_size),
    ] {
        put(&mut p, k, v.map(Value::from));
    }
    if let Some(dlq) = q.dlq.as_deref() {
        let target = if dlq == "auto" {
            let base = format!("{}-dlq", queue_name(q).trim_end_matches(".fifo"));
            let name = if q.fifo { format!("{base}.fifo") } else { base };
            let mut d = Map::new();
            d.insert("QueueName".into(), name.into());
            if q.fifo {
                d.insert("FifoQueue".into(), true.into());
            }
            let dlq_id = format!("{}Dlq", id("Queue", &q.name));
            r.insert(dlq_id.clone(), resource("AWS::SQS::Queue", d));
            json!({ "Fn::GetAtt": [dlq_id, "Arn"] })
        } else {
            arn("Queue", dlq)
        };
        p.insert(
            "RedrivePolicy".into(),
            json!({ "deadLetterTargetArn": target, "maxReceiveCount": q.max_receives.filter(|n| *n > 0).unwrap_or(3) }),
        );
    }
    put(&mut p, "Tags", tags(&q.tags));
    r.insert(id("Queue", &q.name), resource("AWS::SQS::Queue", p));
}

fn topic(r: &mut Resources, t: &Topic) {
    let mut p = Map::new();
    p.insert("TopicName".into(), t.name.clone().into());
    put(&mut p, "Tags", tags(&t.tags));
    let topic_id = id("Topic", &t.name);
    r.insert(topic_id.clone(), resource("AWS::SNS::Topic", p));
    for (i, s) in t.subscribe.iter().enumerate() {
        let (protocol, endpoint) = if let Some(q) = &s.queue {
            ("sqs", arn("Queue", q))
        } else if let Some(f) = &s.lambda {
            ("lambda", arn("Function", f))
        } else {
            let url = s.http.clone().unwrap_or_default();
            let proto = if url.starts_with("https:") {
                "https"
            } else {
                "http"
            };
            (proto, Value::from(url))
        };
        let mut sp = Map::new();
        sp.insert("TopicArn".into(), json!({ "Ref": topic_id }));
        sp.insert("Protocol".into(), protocol.into());
        sp.insert("Endpoint".into(), endpoint);
        if s.raw {
            sp.insert("RawMessageDelivery".into(), true.into());
        }
        if let Some(f) = s.filter.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
            put(&mut sp, "FilterPolicy", serde_json::from_str(f).ok());
        }
        r.insert(
            format!("{topic_id}Sub{i}"),
            resource("AWS::SNS::Subscription", sp),
        );
    }
}

/// `"pk:S sk:N"` as attribute definitions and a key schema.
pub(crate) fn key_parts(key: &str) -> Vec<(String, String)> {
    key.split_whitespace()
        .filter_map(|part| {
            let (n, t) = part.split_once(':')?;
            Some((n.to_owned(), t.to_ascii_uppercase()))
        })
        .collect()
}

fn key_schema(parts: &[(String, String)]) -> Value {
    Value::Array(
        parts
            .iter()
            .zip(["HASH", "RANGE"])
            .map(|((n, _), kt)| json!({ "AttributeName": n, "KeyType": kt }))
            .collect(),
    )
}

fn index(ix: &Index) -> Value {
    let kind = ix
        .projection
        .as_deref()
        .unwrap_or("ALL")
        .to_ascii_uppercase();
    let mut projection = json!({ "ProjectionType": kind });
    if !ix.include.is_empty()
        && let Some(o) = projection.as_object_mut()
    {
        o.insert("NonKeyAttributes".into(), ix.include.clone().into());
    }
    json!({
        "IndexName": ix.name,
        "KeySchema": key_schema(&key_parts(&ix.key)),
        "Projection": projection,
    })
}

fn table(r: &mut Resources, t: &Table) {
    let mut attrs: Vec<(String, String)> = Vec::new();
    let keys = std::iter::once(&t.key).chain(t.gsi.iter().chain(&t.lsi).map(|i| &i.key));
    for (n, ty) in keys.flat_map(|k| key_parts(k)) {
        if !attrs.iter().any(|(a, _)| *a == n) {
            attrs.push((n, ty));
        }
    }
    let mut p = Map::new();
    p.insert("TableName".into(), t.name.clone().into());
    p.insert("BillingMode".into(), "PAY_PER_REQUEST".into());
    p.insert(
        "AttributeDefinitions".into(),
        attrs
            .iter()
            .map(|(n, ty)| json!({ "AttributeName": n, "AttributeType": ty }))
            .collect(),
    );
    p.insert("KeySchema".into(), key_schema(&key_parts(&t.key)));
    if !t.gsi.is_empty() {
        p.insert(
            "GlobalSecondaryIndexes".into(),
            t.gsi.iter().map(index).collect(),
        );
    }
    if !t.lsi.is_empty() {
        p.insert(
            "LocalSecondaryIndexes".into(),
            t.lsi.iter().map(index).collect(),
        );
    }
    if let Some(a) = &t.ttl {
        p.insert(
            "TimeToLiveSpecification".into(),
            json!({ "AttributeName": a, "Enabled": true }),
        );
    }
    put(
        &mut p,
        "DeletionProtectionEnabled",
        t.deletion_protection.map(Value::from),
    );
    put(&mut p, "Tags", tags(&t.tags));
    r.insert(id("Table", &t.name), resource("AWS::DynamoDB::Table", p));
}
