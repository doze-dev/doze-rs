//! The Kinesis poller: it lists the shards of the stream, reads each from the position it holds, and hands the records to
//! [`deliver`](super::streams::deliver), which owns the retry, bisect and failure-destination semantics both stream sources share.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};

use super::Run;
use super::streams::{Delivered, Rec, deliver};

/// A Kinesis record as Lambda delivers it.
fn kinesis_record(r: &Value, shard: &str, arn: &str, region: &str, role: &str) -> Rec {
    let text = |k: &str| {
        r.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let seq = text("SequenceNumber");
    let arrival = r
        .get("ApproximateArrivalTimestamp")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let data = text("Data");
    let doc = json!({
        "kinesis": {
            "kinesisSchemaVersion": "1.0",
            "partitionKey": text("PartitionKey"),
            "sequenceNumber": seq,
            "data": data,
            "approximateArrivalTimestamp": arrival,
        },
        "eventSource": "aws:kinesis",
        "eventVersion": "1.0",
        "eventID": format!("{shard}:{seq}"),
        "eventName": "aws:kinesis:record",
        "invokeIdentityArn": role,
        "awsRegion": region,
        "eventSourceARN": arn,
    });
    // Filters see the data as JSON when it is, and the record's fields at the top as well.
    let decoded = aws_core::base64::decode(&data)
        .and_then(|b| String::from_utf8(b).ok())
        .map_or(Value::Null, |s| {
            serde_json::from_str(&s).unwrap_or(Value::String(s))
        });
    let mut filter = doc.clone();
    if let (Some(f), Some(k)) = (
        filter.as_object_mut(),
        doc.get("kinesis").and_then(Value::as_object),
    ) {
        for (key, v) in k {
            f.entry(key.clone()).or_insert_with(|| v.clone());
        }
        f.insert("data".into(), decoded.clone());
        if let Some(Value::Object(inner)) = f.get_mut("kinesis") {
            inner.insert("data".into(), decoded);
        }
    }
    Rec {
        seq: seq.parse().unwrap_or(0),
        pos: seq,
        doc,
        filter,
        arrived_ms: (arrival * 1000.0) as i64,
    }
}

/// The poller of one Kinesis mapping.
pub(super) async fn kinesis(run: Run, kinesis: Arc<aws_kinesis::Kinesis>) {
    let Some(peers) = run.inner.peers.clone() else {
        return;
    };
    let mut poll = Poll {
        arn: run.source_arn().to_owned(),
        role: run.text("FunctionArn").to_owned(),
        batch: run.int("BatchSize", 100).clamp(1, 10_000),
        iterators: run.seed.clone(),
        peers,
        run,
    };
    let mut appended = kinesis.appended();
    loop {
        if poll.run.stop.is_cancelled() {
            return;
        }
        appended.mark_unchanged();
        let mut any = false;
        for shard in poll.shards().await {
            let records = poll.read(&shard).await;
            if records.is_empty() {
                continue;
            }
            any = true;
            if matches!(
                deliver(&poll.run, "KinesisBatchInfo", &shard, records).await,
                Delivered::Stopped
            ) {
                return;
            }
        }
        if !any {
            tokio::select! {
                changed = appended.changed() => if changed.is_err() { return },
                () = poll.run.stop.cancelled() => return,
            }
        }
    }
}

/// What the Kinesis poller holds: the stream, and where it is in each shard.
struct Poll {
    run: Run,
    peers: Arc<aws_peers::Peers>,
    arn: String,
    role: String,
    batch: i64,
    iterators: HashMap<String, String>,
}

impl Poll {
    async fn kinesis(&self, op: &'static str, body: Value) -> Option<Value> {
        self.peers
            .call("lambda", &self.run.caller, "kinesis", op, &body)
            .await
            .ok()
    }

    /// The shards of the stream, parents before children.
    async fn shards(&self) -> Vec<String> {
        let listed = self
            .kinesis("ListShards", json!({"StreamARN": self.arn}))
            .await;
        listed
            .and_then(|l| l.get("Shards").and_then(Value::as_array).cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|s| s.get("ShardId").and_then(Value::as_str).map(str::to_owned))
            .collect()
    }

    /// The next records of a shard, and the iterator after them kept.
    async fn read(&mut self, shard: &str) -> Vec<Rec> {
        let Some(iterator) = shard_iterator(
            &self.run,
            &self.peers,
            &self.arn,
            shard,
            &mut self.iterators,
        )
        .await
        else {
            return Vec::new();
        };
        let ask = json!({"StreamARN": self.arn, "ShardIterator": iterator, "Limit": self.batch});
        let Some(got) = self.kinesis("GetRecords", ask).await else {
            self.iterators.remove(shard);
            return Vec::new();
        };
        match got.get("NextShardIterator").and_then(Value::as_str) {
            Some(next) => {
                self.iterators.insert(shard.to_owned(), next.to_owned());
            }
            None => {
                self.iterators.remove(shard);
            }
        }
        let records = got
            .get("Records")
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        records
            .map(|r| kinesis_record(r, shard, &self.arn, &self.run.region, &self.role))
            .collect()
    }
}

/// The iterator for a shard: the one held, or one made from the saved position or the mapping's starting position.
pub(super) async fn shard_iterator(
    run: &Run,
    peers: &aws_peers::Peers,
    arn: &str,
    shard: &str,
    held: &mut HashMap<String, String>,
) -> Option<String> {
    if let Some(it) = held.get(shard) {
        return Some(it.clone());
    }
    let mut ask = json!({"StreamARN": arn, "ShardId": shard});
    match run.position(shard).await {
        Some(pos) => {
            crate::set(
                &mut ask,
                "ShardIteratorType",
                json!("AFTER_SEQUENCE_NUMBER"),
            );
            crate::set(&mut ask, "StartingSequenceNumber", json!(pos));
        }
        None => {
            let start = run
                .cfg
                .get("StartingPosition")
                .and_then(Value::as_str)
                .unwrap_or("LATEST");
            crate::set(&mut ask, "ShardIteratorType", json!(start));
            if start == "AT_TIMESTAMP" {
                crate::set(
                    &mut ask,
                    "Timestamp",
                    run.cfg
                        .get("StartingPositionTimestamp")
                        .cloned()
                        .unwrap_or(json!(0)),
                );
            }
        }
    }
    let got = peers
        .call("lambda", &run.caller, "kinesis", "GetShardIterator", &ask)
        .await
        .ok()?;
    let it = got.get("ShardIterator")?.as_str()?.to_owned();
    held.insert(shard.to_owned(), it.clone());
    Some(it)
}
