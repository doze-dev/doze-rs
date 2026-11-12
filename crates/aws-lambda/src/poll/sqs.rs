//! The SQS poller. It reads with `ReceiveMessage` over the peer bus, waits for SQS to say a message arrived when the queue is empty
//! (and, while messages are in flight or delayed and so may come back by the clock alone, looks again each second), filters,
//! invokes the function, and deletes what succeeded: all of a batch, or all but the items the function reports as failed.

use std::sync::Arc;
use std::time::Duration;

use aws_peers::Peers;
use aws_wire::service::Caller;
use serde_json::{Map, Value, json};
use tokio::time::Instant;

use super::Run;
use crate::errors::invalid;

/// The queue's name and account from its ARN.
fn name_of(arn: &str) -> Option<(&str, &str)> {
    let mut it = arn.split(':').skip(4);
    Some((it.next()?, it.next()?))
}

async fn queue_url(
    peers: &Peers,
    acting: &Caller,
    arn: &str,
) -> Result<String, aws_wire::ApiError> {
    let (account, name) =
        name_of(arn).ok_or_else(|| invalid(format!("Invalid event source ARN: {arn}")))?;
    let got = peers
        .call(
            "lambda",
            acting,
            "sqs",
            "GetQueueUrl",
            &json!({"QueueName": name, "QueueOwnerAWSAccountId": account}),
        )
        .await?;
    Ok(got
        .get("QueueUrl")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned())
}

/// The checks made when a mapping to a queue is created or changed: the queue is there, and hides a message for at least as long as
/// the function runs (recorded 2026-11-09).
pub(crate) async fn check(
    peers: &Peers,
    caller: &Caller,
    arn: &str,
    function_timeout: i64,
) -> Result<(), aws_wire::ApiError> {
    let acting = Caller {
        service: Some("lambda".to_owned()),
        ..caller.clone()
    };
    let missing = |e: aws_wire::ApiError| {
        if e.code == "QueueDoesNotExist" || e.message.contains("does not exist") {
            invalid(
                "Error occurred while ReceiveMessage. SQS Error Code: AWS.SimpleQueueService.NonExistentQueue. SQS Error Message: The specified queue does not exist.",
            )
        } else {
            invalid(format!(
                "Error occurred while ReceiveMessage. SQS Error Message: {}",
                e.message
            ))
        }
    };
    let url = queue_url(peers, &acting, arn).await.map_err(missing)?;
    let got = peers
        .call(
            "lambda",
            &acting,
            "sqs",
            "GetQueueAttributes",
            &json!({"QueueUrl": url, "AttributeNames": ["VisibilityTimeout"]}),
        )
        .await?;
    let visibility: i64 = got
        .pointer("/Attributes/VisibilityTimeout")
        .and_then(Value::as_str)
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    if visibility < function_timeout {
        return Err(invalid(format!(
            "Queue visibility timeout: {visibility} seconds is less than Function timeout: {function_timeout} seconds"
        )));
    }
    Ok(())
}

/// An SQS message as Lambda delivers it.
fn record(m: &Value, arn: &str, region: &str) -> Value {
    let text = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or_default();
    let attrs: Map<String, Value> = m
        .get("Attributes")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut message_attributes = Map::new();
    for (name, a) in m
        .get("MessageAttributes")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let mut one = Map::new();
        if let Some(s) = a.get("StringValue") {
            one.insert("stringValue".into(), s.clone());
        }
        if let Some(b) = a.get("BinaryValue") {
            one.insert("binaryValue".into(), b.clone());
        }
        one.insert("stringListValues".into(), json!([]));
        one.insert("binaryListValues".into(), json!([]));
        one.insert(
            "dataType".into(),
            a.get("DataType").cloned().unwrap_or(Value::Null),
        );
        message_attributes.insert(name.clone(), Value::Object(one));
    }
    json!({
        "messageId": text("MessageId"),
        "receiptHandle": text("ReceiptHandle"),
        "body": text("Body"),
        "attributes": attrs,
        "messageAttributes": message_attributes,
        "md5OfBody": text("MD5OfBody"),
        "eventSource": "aws:sqs",
        "eventSourceARN": arn,
        "awsRegion": region,
    })
}

struct Queue {
    url: String,
    arn: String,
}

impl Run {
    async fn sqs(
        &self,
        peers: &Peers,
        op: &'static str,
        body: Value,
    ) -> Result<Value, aws_wire::ApiError> {
        peers.call("lambda", &self.caller, "sqs", op, &body).await
    }

    /// Up to `want` messages: one receive, or several until `window` has passed for a batch that is not yet full.
    async fn gather(&self, peers: &Peers, q: &Queue, sqs: &Arc<aws_sqs::Sqs>) -> Vec<Value> {
        let want = usize::try_from(self.int("BatchSize", 10))
            .unwrap_or(10)
            .max(1);
        let window = Duration::from_secs(
            u64::try_from(self.int("MaximumBatchingWindowInSeconds", 0)).unwrap_or(0),
        );
        let until = Instant::now() + window;
        let mut batch: Vec<Value> = Vec::new();
        loop {
            let waiter = tokio::spawn({
                let sqs = Arc::clone(sqs);
                async move { sqs.message_arrived().await }
            });
            tokio::task::yield_now().await;
            let ask = json!({"QueueUrl": q.url, "MaxNumberOfMessages": (want - batch.len()).min(10),
                "WaitTimeSeconds": 0, "AttributeNames": ["All"], "MessageAttributeNames": ["All"]});
            let got = self.sqs(peers, "ReceiveMessage", ask).await;
            let messages: Vec<Value> = got
                .ok()
                .and_then(|g| g.get("Messages").and_then(Value::as_array).cloned())
                .unwrap_or_default();
            let found = !messages.is_empty();
            batch.extend(messages);
            if batch.len() >= want {
                waiter.abort();
                return batch;
            }
            if found {
                waiter.abort();
                continue;
            }
            // The queue is empty for now: a batch with something in it waits out the window for more.
            if batch.is_empty() || window.is_zero() {
                waiter.abort();
                return batch;
            }
            let left = until.saturating_duration_since(Instant::now());
            tokio::select! {
                _ = waiter => {}
                () = tokio::time::sleep(left) => return batch,
                () = self.stop.cancelled() => return batch,
            }
        }
    }

    async fn has_hidden(&self, peers: &Peers, q: &Queue) -> bool {
        let ask = json!({"QueueUrl": q.url, "AttributeNames": ["ApproximateNumberOfMessagesNotVisible", "ApproximateNumberOfMessagesDelayed"]});
        let Ok(got) = self.sqs(peers, "GetQueueAttributes", ask).await else {
            return false;
        };
        [
            "ApproximateNumberOfMessagesNotVisible",
            "ApproximateNumberOfMessagesDelayed",
        ]
        .iter()
        .any(|k| {
            got.pointer(&format!("/Attributes/{k}"))
                .and_then(Value::as_str)
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0)
                > 0
        })
    }

    async fn delete(&self, peers: &Peers, q: &Queue, handles: &[String]) {
        for chunk in handles.chunks(10) {
            let entries: Vec<Value> = chunk
                .iter()
                .enumerate()
                .map(|(i, h)| json!({"Id": i.to_string(), "ReceiptHandle": h}))
                .collect();
            if let Err(e) = self
                .sqs(
                    peers,
                    "DeleteMessageBatch",
                    json!({"QueueUrl": q.url, "Entries": entries}),
                )
                .await
            {
                tracing::warn!(mapping = %self.uuid, "could not delete messages: {}", e.message);
            }
        }
    }

    /// Filters a batch, invokes the function, and deletes what succeeded.
    async fn handle(&self, peers: &Peers, q: &Queue, messages: Vec<Value>) {
        let handle_of = |m: &Value| {
            m.get("ReceiptHandle")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let records: Vec<(Value, String)> = messages
            .iter()
            .map(|m| (record(m, &q.arn, &self.region), handle_of(m)))
            .collect();
        // What the filters drop is deleted: it is not for the function.
        let (keep, filtered): (Vec<_>, Vec<_>) = records
            .into_iter()
            .partition(|(r, _)| self.passes(&filter_doc(r)));
        let dropped: Vec<String> = filtered.into_iter().map(|(_, h)| h).collect();
        self.delete(peers, q, &dropped).await;
        if keep.is_empty() {
            return;
        }
        let docs: Vec<Value> = keep.iter().map(|(r, _)| r.clone()).collect();
        let done = self.invoke(&docs).await;
        if done.failed {
            if done.ran {
                self.note("FAILED").await;
            }
            self.pause(Duration::from_millis(500)).await;
            return;
        }
        let failed_ids = if self.reports_failures() {
            failures(&done.payload, &keep)
        } else {
            Some(Vec::new())
        };
        let Some(failed_ids) = failed_ids else {
            // An answer that is not a valid report fails the whole batch.
            self.note("FAILED").await;
            return;
        };
        let ok: Vec<String> = keep
            .iter()
            .filter(|(r, _)| {
                !failed_ids
                    .iter()
                    .any(|f| Some(f.as_str()) == r.get("messageId").and_then(Value::as_str))
            })
            .map(|(_, h)| h.clone())
            .collect();
        self.delete(peers, q, &ok).await;
        self.note(if failed_ids.is_empty() {
            "OK"
        } else {
            "PARTIAL"
        })
        .await;
    }
}

/// What filters see of a message: the record, with a body that is a JSON object read as one.
fn filter_doc(record: &Value) -> Value {
    let mut doc = record.clone();
    let parsed = record
        .get("body")
        .and_then(Value::as_str)
        .and_then(|b| serde_json::from_str::<Value>(b).ok())
        .filter(Value::is_object);
    if let Some(body) = parsed {
        crate::set(&mut doc, "body", body);
    }
    doc
}

/// The ids a function reports as failed: `None` when its answer is not a valid report (the whole batch failed).
fn failures(payload: &[u8], batch: &[(Value, String)]) -> Option<Vec<String>> {
    if payload.iter().all(u8::is_ascii_whitespace) {
        return Some(Vec::new());
    }
    let v: Value = serde_json::from_slice(payload).ok()?;
    if v.is_null() {
        return Some(Vec::new());
    }
    let list = match v.get("batchItemFailures") {
        None | Some(Value::Null) => return Some(Vec::new()),
        Some(Value::Array(a)) => a,
        Some(_) => return None,
    };
    let mut ids = Vec::new();
    for item in list {
        let id = item
            .get("itemIdentifier")?
            .as_str()
            .filter(|s| !s.is_empty())?;
        batch
            .iter()
            .any(|(r, _)| r.get("messageId").and_then(Value::as_str) == Some(id))
            .then_some(())?;
        ids.push(id.to_owned());
    }
    Some(ids)
}

/// The poller of one SQS mapping.
pub(crate) async fn run(run: Run, sqs: Arc<aws_sqs::Sqs>) {
    let Some(peers) = run.inner.peers.clone() else {
        return;
    };
    let arn = run.source_arn().to_owned();
    let url = loop {
        match queue_url(&peers, &run.caller, &arn).await {
            Ok(u) => break u,
            Err(_) => {
                // The queue is not there (yet): look again in a while, and stop when the mapping does.
                if !run.pause(Duration::from_secs(5)).await {
                    return;
                }
            }
        }
    };
    let q = Queue { url, arn };
    loop {
        if run.stop.is_cancelled() {
            return;
        }
        let waiter = tokio::spawn({
            let sqs = Arc::clone(&sqs);
            async move { sqs.message_arrived().await }
        });
        tokio::task::yield_now().await;
        let batch = run.gather(&peers, &q, &sqs).await;
        if !batch.is_empty() {
            waiter.abort();
            run.handle(&peers, &q, batch).await;
            continue;
        }
        let hidden = run.has_hidden(&peers, &q).await;
        let wake = tokio::time::sleep(Duration::from_secs(1));
        tokio::select! {
            _ = waiter => {}
            () = wake, if hidden => {}
            () = run.stop.cancelled() => return,
        }
    }
}
