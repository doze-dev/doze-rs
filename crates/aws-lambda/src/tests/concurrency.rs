//! Concurrency of event source mappings: SQS batches run up to `ScalingConfig.MaximumConcurrency` at once, and a stream's
//! `ParallelizationFactor` runs a shard's partition keys in parallel lanes while each key keeps its order.

use aws_core::base64;
use serde_json::json;

use super::sources::{logged, queue};
use super::{World, have};

/// Marks itself in flight in the directory its message names, waits, and prints how many were in flight with it.
const OVERLAP: &[u8] = b"import os, time, uuid\ndef handler(event, context):\n    d = event['Records'][0]['body']\n    me = os.path.join(d, uuid.uuid4().hex)\n    open(me, 'w').close()\n    time.sleep(0.8)\n    print('SEEN', len(os.listdir(d)))\n    os.remove(me)\n    return {}\n";

/// Prints each record's key and data, in the order it was given them.
const KEYS: &[u8] = b"import base64\ndef handler(event, context):\n    for r in event['Records']:\n        print('REC', r['kinesis']['partitionKey'], base64.b64decode(r['kinesis']['data']).decode())\n    return {}\n";

#[tokio::test]
async fn sqs_batches_run_together_up_to_the_maximum_concurrency() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    w.create(
        "o",
        "python3.12",
        "index.handler",
        &[("index.py", OVERLAP, 0o644)],
    )
    .await;
    let (url, arn) = queue(&w, "q", "30").await;
    let dir = aws_testkit::TempDir::new("lambda-overlap");
    let path = dir.path().to_string_lossy().into_owned();
    w.ok(
        "CreateEventSourceMapping",
        json!({"FunctionName": "o", "EventSourceArn": arn, "BatchSize": 1, "ScalingConfig": {"MaximumConcurrency": 2}}),
    )
    .await;
    for _ in 0..6 {
        w.peer(
            "sqs",
            "SendMessage",
            json!({"QueueUrl": url, "MessageBody": path}),
        )
        .await;
    }
    w.eventually("six batches", || async {
        logged(&w, "o", "SEEN").await.len() >= 6
    })
    .await;
    let seen: Vec<u32> = logged(&w, "o", "SEEN")
        .await
        .iter()
        .filter_map(|l| l.strip_prefix("SEEN ")?.parse().ok())
        .collect();
    assert!(
        seen.iter().all(|n| *n <= 2),
        "never more than the maximum: {seen:?}"
    );
    assert!(seen.contains(&2), "two at once: {seen:?}");
    w.stop().await;
}

#[tokio::test]
async fn a_parallelization_factor_keeps_each_partition_key_in_order() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    w.create(
        "k",
        "python3.12",
        "index.handler",
        &[("index.py", KEYS, 0o644)],
    )
    .await;
    w.peer(
        "kinesis",
        "CreateStream",
        json!({"StreamName": "s", "ShardCount": 1}),
    )
    .await;
    let described = w
        .peer(
            "kinesis",
            "DescribeStreamSummary",
            json!({"StreamName": "s"}),
        )
        .await;
    let stream_arn = described["StreamDescriptionSummary"]["StreamARN"]
        .as_str()
        .unwrap()
        .to_owned();
    for i in 0..30 {
        let key = format!("key{}", i % 5);
        w.peer(
            "kinesis",
            "PutRecord",
            json!({"StreamName": "s", "PartitionKey": key, "Data": base64::encode(i.to_string().as_bytes())}),
        )
        .await;
    }
    w.ok(
        "CreateEventSourceMapping",
        json!({"FunctionName": "k", "EventSourceArn": stream_arn, "StartingPosition": "TRIM_HORIZON",
            "BatchSize": 2, "ParallelizationFactor": 3}),
    )
    .await;
    w.eventually("thirty records", || async {
        logged(&w, "k", "REC").await.len() >= 30
    })
    .await;
    let lines = logged(&w, "k", "REC").await;
    assert_eq!(lines.len(), 30, "each once");
    for k in 0..5 {
        let key = format!("key{k}");
        let order: Vec<u32> = lines
            .iter()
            .filter_map(|l| {
                let mut parts = l.split(' ').skip(1);
                (parts.next()? == key).then(|| parts.next()?.parse().ok())?
            })
            .collect();
        assert!(
            order.windows(2).all(|p| p[0] < p[1]),
            "{key} in order: {order:?}"
        );
        assert_eq!(order.len(), 6);
    }
    w.stop().await;
}
