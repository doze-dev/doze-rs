//! SQS from Go's `sqs/sqs.bolt` (sqs/store.go in the Go doze-aws): bucket `queues` (name → queue JSON) and
//! `q:<queue>` (`be64(seq)` → message JSON, in the order they were sent).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::Replayed;
use crate::client::Client;

#[derive(Deserialize)]
struct GoQueue {
    name: String,
    #[serde(default)]
    fifo: bool,
    #[serde(default)]
    content_based_dedup: bool,
    #[serde(default)]
    visibility_timeout: i64,
    #[serde(default)]
    delay_seconds: i64,
    #[serde(default)]
    retention_period: i64,
    #[serde(default)]
    max_message_size: i64,
    #[serde(default)]
    wait_time_seconds: i64,
    #[serde(default)]
    dead_letter_target: String,
    #[serde(default)]
    max_receive_count: i64,
    #[serde(default)]
    attrs: BTreeMap<String, String>,
    #[serde(default)]
    tags: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct GoAttr {
    #[serde(default)]
    data_type: String,
    #[serde(default)]
    string_value: String,
    /// Base64, as Go's JSON writes a byte slice and as the SQS JSON protocol carries one.
    #[serde(default)]
    binary_value: Option<String>,
}

#[derive(Deserialize)]
struct GoMessage {
    #[serde(default)]
    id: String,
    body: String,
    #[serde(default)]
    attrs: BTreeMap<String, GoAttr>,
    #[serde(default)]
    sys_attrs: BTreeMap<String, GoAttr>,
    #[serde(default)]
    group_id: String,
    #[serde(default)]
    dedup_id: String,
}

/// One SQS call (JSON 1.0); its answer.
fn call(client: &Client, op: &str, body: &Value) -> Result<Value, String> {
    let headers = [
        (
            "content-type".to_owned(),
            "application/x-amz-json-1.0".to_owned(),
        ),
        ("x-amz-target".to_owned(), format!("AmazonSQS.{op}")),
    ];
    let reply = client.signed("sqs", "POST", "/", &headers, body.to_string().as_bytes())?;
    if !(200..300).contains(&reply.status) {
        return Err(format!(
            "SQS {op}: {} {}",
            reply.status,
            String::from_utf8_lossy(&reply.body)
        ));
    }
    Ok(serde_json::from_slice(&reply.body).unwrap_or(Value::Null))
}

/// `v`'s value at `path` (object keys), as text.
fn text<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(v, |v, k| v.get(*k))?.as_str()
}

fn url(client: &Client, name: &str) -> Result<String, String> {
    let got = call(client, "GetQueueUrl", &json!({ "QueueName": name }))?;
    text(&got, &["QueueUrl"])
        .map(str::to_owned)
        .ok_or_else(|| format!("no URL for queue {name}"))
}

fn attrs(m: &BTreeMap<String, GoAttr>) -> Value {
    let mut out = Map::new();
    for (k, a) in m {
        let mut v = Map::new();
        v.insert("DataType".into(), json!(a.data_type));
        match &a.binary_value {
            Some(b) if a.data_type.starts_with("Binary") => {
                v.insert("BinaryValue".into(), json!(b))
            }
            _ => v.insert("StringValue".into(), json!(a.string_value)),
        };
        out.insert(k.clone(), Value::Object(v));
    }
    Value::Object(out)
}

/// Replays the queues and messages in `dir` (Go's `sqs/`), skipping queues named `sqs:<queue>` in `finished`.
pub(super) fn replay(
    dir: &Path,
    client: &Client,
    finished: &[String],
    r: &mut Replayed,
    mark: &dyn Fn(&str) -> Result<(), String>,
) -> Result<(), String> {
    let file = dir.join("sqs.bolt");
    if !file.is_file() {
        return Ok(());
    }
    let db = doze_bolt::Db::open(&file).map_err(|e| format!("sqs/sqs.bolt: {e}"))?;
    let listed = call(client, "ListQueues", &json!({ "MaxResults": 1000 }))?;
    let there: Vec<String> = listed
        .get("QueueUrls")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|u| u.as_str()?.rsplit('/').next().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let mut queues = Vec::new();
    for (_, value) in db.values(&[b"queues"]).map_err(|e| e.to_string())? {
        let q: GoQueue = serde_json::from_slice(&value).map_err(|e| format!("a queue: {e}"))?;
        queues.push(q);
    }
    // Made first, all of them, so a redrive can name its dead-letter queue; one doze.hcl declares is the stack's.
    let made: Vec<String> = queues
        .iter()
        .filter(|q| !there.contains(&q.name) && !finished.contains(&format!("sqs:{}", q.name)))
        .map(|q| q.name.clone())
        .collect();
    for q in queues.iter().filter(|q| made.contains(&q.name)) {
        create(client, q)?;
    }
    for q in &queues {
        let done = format!("sqs:{}", q.name);
        if finished.contains(&done) {
            continue;
        }
        let at = url(client, &q.name)?;
        if made.contains(&q.name) && !q.dead_letter_target.is_empty() {
            redrive(client, q, &at)?;
        }
        r.messages += messages(&db, client, q, &at)?;
        r.queues += 1;
        mark(&done)?;
    }
    Ok(())
}

/// Makes a queue Go had and doze.hcl does not declare, with Go's settings (its redrive comes after).
fn create(client: &Client, q: &GoQueue) -> Result<(), String> {
    let mut a: BTreeMap<String, String> = q.attrs.clone();
    for (k, v) in [
        ("VisibilityTimeout", q.visibility_timeout),
        ("DelaySeconds", q.delay_seconds),
        ("MessageRetentionPeriod", q.retention_period),
        ("MaximumMessageSize", q.max_message_size),
        ("ReceiveMessageWaitTimeSeconds", q.wait_time_seconds),
    ] {
        a.insert(k.into(), v.to_string());
    }
    if q.fifo {
        a.insert("FifoQueue".into(), "true".into());
        a.insert(
            "ContentBasedDeduplication".into(),
            q.content_based_dedup.to_string(),
        );
    }
    a.remove("RedrivePolicy");
    call(
        client,
        "CreateQueue",
        &json!({ "QueueName": q.name, "Attributes": a, "tags": q.tags }),
    )?;
    Ok(())
}

/// Points `q` (at `at`) at its dead-letter queue, by the ARN this server gives that queue.
fn redrive(client: &Client, q: &GoQueue, at: &str) -> Result<(), String> {
    let dlq = url(client, &q.dead_letter_target)?;
    let got = call(
        client,
        "GetQueueAttributes",
        &json!({ "QueueUrl": dlq, "AttributeNames": ["QueueArn"] }),
    )?;
    let arn = text(&got, &["Attributes", "QueueArn"]).unwrap_or_default();
    let policy =
        json!({ "deadLetterTargetArn": arn, "maxReceiveCount": q.max_receive_count }).to_string();
    call(
        client,
        "SetQueueAttributes",
        &json!({ "QueueUrl": at, "Attributes": { "RedrivePolicy": policy } }),
    )?;
    Ok(())
}

/// Sends `q`'s messages again, in the order Go kept them; how many.
fn messages(db: &doze_bolt::Db, client: &Client, q: &GoQueue, at: &str) -> Result<usize, String> {
    let mut n = 0;
    for (_, value) in db
        .values(&[format!("q:{}", q.name).as_bytes()])
        .map_err(|e| e.to_string())?
    {
        let m: GoMessage =
            serde_json::from_slice(&value).map_err(|e| format!("a message in {}: {e}", q.name))?;
        let mut send = Map::new();
        send.insert("QueueUrl".into(), json!(at));
        send.insert("MessageBody".into(), json!(m.body));
        if !m.attrs.is_empty() {
            send.insert("MessageAttributes".into(), attrs(&m.attrs));
        }
        if !m.sys_attrs.is_empty() {
            send.insert("MessageSystemAttributes".into(), attrs(&m.sys_attrs));
        }
        if q.fifo {
            send.insert("MessageGroupId".into(), json!(m.group_id));
            // Go's own deduplication id, else the message's: content-based deduplication must not merge two messages
            // Go kept apart.
            let dedup = if m.dedup_id.is_empty() {
                &m.id
            } else {
                &m.dedup_id
            };
            send.insert("MessageDeduplicationId".into(), json!(dedup));
        } else {
            send.insert("DelaySeconds".into(), json!(0));
        }
        call(client, "SendMessage", &Value::Object(send))?;
        n += 1;
    }
    Ok(n)
}
