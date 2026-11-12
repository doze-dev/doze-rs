//! The Kinesis and DynamoDB stream pollers. They read records after the position they last saved, wait for the source to say records
//! arrived when they have caught up, and hand a batch to [`deliver`], which owns the semantics both share: a failed batch is
//! retried (with a growing pause) up to `MaximumRetryAttempts`, split in two when `BisectBatchOnFunctionError` is set, dropped
//! when its records are older than `MaximumRecordAgeInSeconds`, and sent to the failure destination when it gives up; a function
//! that reports item failures has the batch retried from the first failed record.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use aws_core::time::format_iso8601;
use serde_json::{Value, json};

use super::Run;

/// One record of a stream: where it is, its event, and when it arrived.
#[derive(Clone, Debug)]
pub(super) struct Rec {
    /// The sequence number as text, and as a number to compare.
    pos: String,
    seq: u128,
    doc: Value,
    /// What filters see: the event with the record's data decoded.
    filter: Value,
    arrived_ms: i64,
}

/// How a batch ended up.
enum Delivered {
    Done,
    Stopped,
}

/// The pause before attempt `n + 1`: a second, doubling, up to a minute.
fn backoff(attempt: i64) -> Duration {
    Duration::from_secs(1_u64 << attempt.clamp(0, 5) as u32)
}

/// The batch description a failure destination receives.
/// What a failed batch is described by.
struct Failure<'a> {
    kind: &'a str,
    shard: &'a str,
    batch: &'a [Rec],
    condition: &'a str,
    attempts: i64,
}

fn failure_doc(run: &Run, f: &Failure<'_>, last: &super::Invoked) -> Value {
    let Failure {
        kind,
        shard,
        batch,
        condition,
        attempts,
    } = *f;
    let first = batch.first();
    let end = batch.last();
    json!({
        "requestContext": {"requestId": last.request_id, "functionArn": run.text("FunctionArn"), "condition": condition, "approximateInvokeCount": attempts},
        "responseContext": {"statusCode": 200, "executedVersion": last.executed, "functionError": "Unhandled"},
        "version": "1.0",
        "timestamp": format!("{}Z", format_iso8601(run.inner.clock.now_ms() / 1000).trim_end_matches('Z')),
        kind: {
            "shardId": shard,
            "startSequenceNumber": first.map(|r| r.pos.clone()),
            "endSequenceNumber": end.map(|r| r.pos.clone()),
            "approximateArrivalOfFirstRecord": first.map(|r| format_iso8601(r.arrived_ms / 1000)),
            "approximateArrivalOfLastRecord": end.map(|r| format_iso8601(r.arrived_ms / 1000)),
            "batchSize": batch.len(),
            "streamArn": run.source_arn(),
        },
    })
}

/// The first failed record's sequence number the function reported, if it reported any; `Err` for a report that is not valid.
fn first_failure(payload: &[u8], batch: &[Rec]) -> Result<Option<u128>, ()> {
    if payload.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    let v: Value = serde_json::from_slice(payload).map_err(|_| ())?;
    let list = match v.get("batchItemFailures") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(()),
    };
    let mut lowest: Option<u128> = None;
    for item in list {
        let id = item
            .get("itemIdentifier")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or(())?;
        let rec = batch.iter().find(|r| r.pos == id).ok_or(())?;
        lowest = Some(lowest.map_or(rec.seq, |l| l.min(rec.seq)));
    }
    Ok(lowest)
}

/// Delivers `records` of one shard until each has succeeded, been skipped, or been given up on; saves the position after each part.
async fn deliver(run: &Run, kind: &str, shard: &str, records: Vec<Rec>) -> Delivered {
    let retries = run.int("MaximumRetryAttempts", -1);
    let max_age = run.int("MaximumRecordAgeInSeconds", -1);
    let bisect = run
        .cfg
        .get("BisectBatchOnFunctionError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut stack = vec![records];
    while let Some(mut batch) = stack.pop() {
        let Some(last_pos) = batch.last().map(|r| r.pos.clone()) else {
            continue;
        };
        if max_age >= 0 {
            let now = run.inner.clock.now_ms();
            batch.retain(|r| (now - r.arrived_ms) / 1000 <= max_age);
        }
        batch.retain(|r| run.passes(&r.filter));
        let mut attempt = 0_i64;
        let mut split = false;
        while !batch.is_empty() {
            attempt += 1;
            let docs: Vec<Value> = batch.iter().map(|r| r.doc.clone()).collect();
            let done = run.invoke(&docs).await;
            if !done.failed {
                match (run.reports_failures(), first_failure(&done.payload, &batch)) {
                    (true, Ok(Some(from))) => {
                        // Retry from the first record the function could not handle.
                        batch.retain(|r| r.seq >= from);
                        run.note("PARTIAL").await;
                    }
                    (true, Err(())) => run.note("FAILED").await,
                    _ => {
                        run.note("OK").await;
                        break;
                    }
                }
            } else if done.ran {
                run.note("FAILED").await;
            }
            if bisect && batch.len() > 1 {
                let second = batch.split_off(batch.len() / 2);
                stack.push(second);
                stack.push(batch);
                split = true;
                break;
            }
            if retries >= 0 && attempt > retries {
                let failure = Failure {
                    kind,
                    shard,
                    batch: &batch,
                    condition: "RetryAttemptsExhausted",
                    attempts: attempt,
                };
                let doc = failure_doc(run, &failure, &done);
                run.on_failure(&doc).await;
                break;
            }
            if !run.pause(backoff(attempt)).await {
                return Delivered::Stopped;
            }
        }
        // The batch (or the half of it) is over: the position moves past it, so a restart does not read it again.
        if !split {
            run.save_position(shard, &last_pos).await;
        }
    }
    Delivered::Done
}

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
async fn shard_iterator(
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

/// The poller of one DynamoDB stream mapping.
pub(super) async fn dynamodb(run: Run, ddb: Arc<aws_dynamodb::Dynamodb>) {
    let arn = run.source_arn().to_owned();
    let batch = usize::try_from(run.int("BatchSize", 100))
        .unwrap_or(100)
        .clamp(1, 10_000);
    let stream = "stream";
    let mut after: u64 = match run.position(stream).await.and_then(|p| p.parse().ok()) {
        Some(p) => p,
        None => start_of(&run, &ddb, &arn).await,
    };
    loop {
        if run.stop.is_cancelled() {
            return;
        }
        let waiter = tokio::spawn({
            let ddb = Arc::clone(&ddb);
            async move { ddb.stream_arrived().await }
        });
        tokio::task::yield_now().await;
        let read = ddb.stream_records(&run.region, &arn, after, batch).await;
        let records: Vec<Rec> = read
            .map(|b| {
                b.records
                    .iter()
                    .map(|r| {
                        let mut doc = r.to_event(&run.region);
                        crate::set(&mut doc, "eventSourceARN", json!(arn));
                        Rec {
                            pos: r.seq.to_string(),
                            seq: u128::from(r.seq),
                            filter: doc.clone(),
                            doc,
                            arrived_ms: r.time_ms,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        if let Some(last) = records.last() {
            waiter.abort();
            after = u64::try_from(last.seq).unwrap_or(after);
            if matches!(
                deliver(&run, "DDBStreamBatchInfo", stream, records).await,
                Delivered::Stopped
            ) {
                return;
            }
            continue;
        }
        tokio::select! {
            _ = waiter => {}
            () = run.stop.cancelled() => return,
        }
    }
}

/// What a mapping needs settled before the call that made it returns: where a DynamoDB stream starts, and the iterators of a Kinesis
/// stream's shards that have no saved position. A record put right after the mapping is made must not be missed because the poller
/// task had not run yet.
pub(super) async fn prepare(mut run: Run) -> Run {
    let Some(sources) = run.inner.sources.get().cloned() else {
        return run;
    };
    match crate::mappings::Source::of(run.source_arn()) {
        Some(crate::mappings::Source::DynamoDb) => {
            if run.position("stream").await.is_none() {
                start_of(&run, &sources.dynamodb, run.source_arn()).await;
            }
        }
        Some(crate::mappings::Source::Kinesis) => {
            if let Some(peers) = run.inner.peers.clone() {
                let arn = run.source_arn().to_owned();
                let listed = peers
                    .call(
                        "lambda",
                        &run.caller,
                        "kinesis",
                        "ListShards",
                        &json!({"StreamARN": arn}),
                    )
                    .await;
                let shards: Vec<String> = listed
                    .ok()
                    .and_then(|l| l.get("Shards").and_then(Value::as_array).cloned())
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|s| s.get("ShardId").and_then(Value::as_str).map(str::to_owned))
                    .collect();
                let mut seed = HashMap::new();
                for shard in shards {
                    shard_iterator(&run, &peers, &arn, &shard, &mut seed).await;
                }
                run.seed = seed;
            }
        }
        _ => {}
    }
    run
}

/// Where a mapping with no saved position starts: the beginning, or after the newest record there is now. Saved at once, so a restart
/// starts from the same place.
async fn start_of(run: &Run, ddb: &aws_dynamodb::Dynamodb, arn: &str) -> u64 {
    let latest = run.cfg.get("StartingPosition").and_then(Value::as_str) != Some("TRIM_HORIZON");
    let start = if latest {
        ddb.stream_records(&run.region, arn, 0, usize::MAX)
            .await
            .ok()
            .and_then(|b| b.records.last().map(|r| r.seq))
            .unwrap_or(0)
    } else {
        0
    };
    run.save_position("stream", &start.to_string()).await;
    start
}
