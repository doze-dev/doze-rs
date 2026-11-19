//! `SubscribeToShard` end to end: the service with its actor, the subscription task and the frames a client reads.

use std::sync::Arc;
use std::time::Duration;

use aws_testkit::Harness;
use aws_wire::rest::Download;
use aws_wire::service::Service;
use doze_runtime::{Ctx, ManualClock, Tasks};
use serde_json::{Value, json};

use super::temp;
use crate::Kinesis;

/// A Kinesis service on disk, and a harness that builds its calls.
struct Live {
    k: Kinesis,
    h: Harness<Kinesis>,
    clock: Arc<ManualClock>,
    _dir: aws_testkit::TempDir,
}

/// One frame as a client reads it: its type (event or exception) and payload.
#[derive(Debug)]
struct Frame {
    kind: String,
    payload: Value,
}

fn ctx(dir: &std::path::Path, clock: Arc<ManualClock>) -> Arc<Ctx> {
    Arc::new(Ctx {
        dir: dir.to_path_buf(),
        idle: Duration::from_secs(300),
        tasks: Tasks::new(),
        clock,
        ext: None,
    })
}

impl Live {
    fn new() -> Self {
        let dir = temp();
        let clock = Arc::new(ManualClock::at(1_700_000_000_000));
        let k = Kinesis::new(ctx(dir.path(), clock.clone()));
        let h = Harness::new(Kinesis::new(ctx(&dir.path().join("unused"), clock.clone())));
        Self {
            k,
            h,
            clock,
            _dir: dir,
        }
    }

    async fn ok(&self, op: &str, request: Value) -> Value {
        let ix = self.h.op(op);
        let out = self.k.call(self.h.call(ix, &request).unwrap()).await;
        let out = out.unwrap_or_else(|e| panic!("{op}: {} {}", e.code, e.message));
        self.h.json(ix, &out)
    }

    async fn subscribe(&self, request: Value) -> Result<Download, aws_wire::ApiError> {
        let call = self.h.call(self.h.op("SubscribeToShard"), &request)?;
        self.k.call_stream(call).await
    }

    /// A stream with one shard and a registered consumer: the consumer's ARN.
    async fn consumer(&self) -> String {
        self.ok("CreateStream", json!({"StreamName": "s", "ShardCount": 1}))
            .await;
        let arn = "arn:aws:kinesis:us-east-1:000000000000:stream/s";
        let made = self
            .ok(
                "RegisterStreamConsumer",
                json!({"StreamARN": arn, "ConsumerName": "c"}),
            )
            .await;
        made["Consumer"]["ConsumerARN"].as_str().unwrap().to_owned()
    }

    async fn put(&self, data: &str) -> String {
        let r = self
            .ok(
                "PutRecord",
                json!({"StreamName": "s", "PartitionKey": "k", "Data": aws_core::base64::encode(data.as_bytes())}),
            )
            .await;
        r["SequenceNumber"].as_str().unwrap().to_owned()
    }
}

fn from(consumer: &str, position: Value) -> Value {
    json!({"ConsumerARN": consumer, "ShardId": "shardId-000000000000", "StartingPosition": position})
}

/// The next frame, or `None` when the stream has ended.
async fn next(stream: &mut Download) -> Option<Frame> {
    let bytes = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx))
        .await?
        .unwrap();
    let be32 = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let (total, heads) = (be32(0), be32(4));
    let mut at = 12;
    let mut kind = String::new();
    while at < 12 + heads {
        let n = bytes[at] as usize;
        let name = std::str::from_utf8(&bytes[at + 1..at + 1 + n])
            .unwrap()
            .to_owned();
        at += 1 + n + 1;
        let len = u16::from_be_bytes([bytes[at], bytes[at + 1]]) as usize;
        let value = std::str::from_utf8(&bytes[at + 2..at + 2 + len])
            .unwrap()
            .to_owned();
        at += 2 + len;
        if name == ":event-type" || name == ":exception-type" {
            kind = value;
        }
    }
    let payload = serde_json::from_slice(&bytes[12 + heads..total - 4]).unwrap();
    Some(Frame { kind, payload })
}

/// The next `SubscribeToShardEvent`, past the `initial-response`.
async fn event(stream: &mut Download) -> Value {
    loop {
        let f = tokio::time::timeout(Duration::from_secs(10), next(stream))
            .await
            .expect("an event in time")
            .expect("the stream is open");
        if f.kind == "SubscribeToShardEvent" {
            return f.payload;
        }
        assert_eq!(f.kind, "initial-response");
    }
}

fn data(event: &Value) -> Vec<String> {
    event["Records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let raw = aws_core::base64::decode(r["Data"].as_str().unwrap()).unwrap();
            String::from_utf8(raw).unwrap()
        })
        .collect()
}

#[tokio::test]
async fn records_already_there_and_records_that_arrive_are_pushed() {
    let w = Live::new();
    let consumer = w.consumer().await;
    w.put("one").await;
    let mut s = w
        .subscribe(from(&consumer, json!({"Type": "TRIM_HORIZON"})))
        .await
        .unwrap();
    let first = event(&mut s).await;
    assert_eq!(data(&first), ["one"]);
    assert_eq!(first["MillisBehindLatest"], 0);
    let seq = first["Records"][0]["SequenceNumber"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(first["ContinuationSequenceNumber"], seq.as_str());
    w.put("two").await;
    let pushed = event(&mut s).await;
    assert_eq!(data(&pushed), ["two"]);
    assert_eq!(w.k.subs.open(), 1);
    drop(s);
}

#[tokio::test]
async fn every_starting_position_is_honoured() {
    let w = Live::new();
    let consumer = w.consumer().await;
    let a = w.put("a").await;
    w.clock.advance(2000);
    w.put("b").await;
    let cases = [
        (
            json!({"Type": "AT_SEQUENCE_NUMBER", "SequenceNumber": a}),
            vec!["a", "b"],
        ),
        (
            json!({"Type": "AFTER_SEQUENCE_NUMBER", "SequenceNumber": a}),
            vec!["b"],
        ),
        (
            json!({"Type": "AT_TIMESTAMP", "Timestamp": 1_700_000_001.0}),
            vec!["b"],
        ),
        (json!({"Type": "LATEST"}), vec![]),
    ];
    for (position, want) in cases {
        // A subscription that is taken over within five seconds is refused, so each case starts later.
        w.clock.advance(6000);
        let mut s = w
            .subscribe(from(&consumer, position.clone()))
            .await
            .unwrap();
        let e = event(&mut s).await;
        assert_eq!(data(&e), want, "{position}");
        // An empty event still says where to continue from: just before the next record.
        assert!(e["ContinuationSequenceNumber"].is_string(), "{position}");
    }
}

#[tokio::test]
async fn a_second_subscription_takes_over_even_at_once() {
    // Recorded 2026-11-19: AWS accepted a second call made at once (the model says one within 5 seconds is refused).
    let w = Live::new();
    let consumer = w.consumer().await;
    let mut first = w
        .subscribe(from(&consumer, json!({"Type": "LATEST"})))
        .await
        .unwrap();
    event(&mut first).await;
    w.clock.advance(1000);
    let mut second = w
        .subscribe(from(&consumer, json!({"Type": "LATEST"})))
        .await
        .unwrap();
    event(&mut second).await;
    let ended = tokio::time::timeout(Duration::from_secs(10), next(&mut first))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ended.kind, "ResourceInUseException");
    assert!(
        next(&mut first).await.is_none(),
        "the old subscription has ended"
    );
}

#[tokio::test]
async fn refusals() {
    let w = Live::new();
    let consumer = w.consumer().await;
    let code = |r: Result<Download, aws_wire::ApiError>| r.err().unwrap().code.to_string();
    let gone = consumer.replace("/consumer/c:", "/consumer/x:");
    assert_eq!(
        code(w.subscribe(from(&gone, json!({"Type": "LATEST"}))).await),
        "ResourceNotFoundException"
    );
    let mut absent = from(&consumer, json!({"Type": "LATEST"}));
    absent["ShardId"] = json!("shardId-000000000009");
    assert_eq!(code(w.subscribe(absent).await), "ResourceNotFoundException");
    assert_eq!(
        code(
            w.subscribe(from(&consumer, json!({"Type": "AT_SEQUENCE_NUMBER"})))
                .await
        ),
        "InvalidArgumentException"
    );
    let mut dry = from(&consumer, json!({"Type": "LATEST"}));
    dry["DryRun"] = json!(true);
    assert_eq!(code(w.subscribe(dry).await), "DryRunOperationException");
}

#[tokio::test]
async fn a_closed_shard_ends_the_subscription_with_its_children() {
    let w = Live::new();
    let consumer = w.consumer().await;
    w.put("before").await;
    let mid = (u128::MAX / 2).to_string();
    w.ok(
        "SplitShard",
        json!({"StreamName": "s", "ShardToSplit": "shardId-000000000000", "NewStartingHashKey": mid}),
    )
    .await;
    let mut s = w
        .subscribe(from(&consumer, json!({"Type": "TRIM_HORIZON"})))
        .await
        .unwrap();
    let e = event(&mut s).await;
    assert_eq!(data(&e), ["before"]);
    assert_eq!(e["ChildShards"].as_array().unwrap().len(), 2);
    assert!(e.get("ContinuationSequenceNumber").is_none());
    assert!(
        next(&mut s).await.is_none(),
        "the subscription ends with the shard"
    );
}

#[tokio::test]
async fn a_deregistered_consumer_ends_its_subscription() {
    let w = Live::new();
    let consumer = w.consumer().await;
    let mut s = w
        .subscribe(from(&consumer, json!({"Type": "LATEST"})))
        .await
        .unwrap();
    event(&mut s).await;
    w.ok("DeregisterStreamConsumer", json!({"ConsumerARN": consumer}))
        .await;
    // The next heartbeat's read finds the consumer gone.
    let f = tokio::time::timeout(Duration::from_secs(15), next(&mut s))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(f.kind, "ResourceNotFoundException");
}

#[tokio::test(start_paused = true)]
async fn a_subscription_ends_after_five_minutes_with_a_heartbeat_between() {
    let w = Live::new();
    let consumer = w.consumer().await;
    let mut s = w
        .subscribe(from(&consumer, json!({"Type": "LATEST"})))
        .await
        .unwrap();
    let started = tokio::time::Instant::now();
    let mut events = 0;
    while let Some(f) = next(&mut s).await {
        if f.kind == "SubscribeToShardEvent" {
            events += 1;
        }
    }
    let lasted = started.elapsed();
    assert!(
        lasted >= Duration::from_secs(300) && lasted < Duration::from_secs(306),
        "{lasted:?}"
    );
    // One at once, then one every five seconds.
    assert!((60..=61).contains(&events), "{events}");
    assert_eq!(w.k.subs.open(), 0, "the claim is released");
}
