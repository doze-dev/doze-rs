//! A Kinesis mapping across a split: the parent is read to its end before its children, and the children start at their trim horizon,
//! so the records of one partition key arrive in order and none written to a child before the poller found it is skipped.

use aws_core::base64;
use serde_json::json;

use super::sources::{PRINTER, logged};
use super::{World, have};

#[tokio::test]
async fn a_split_keeps_each_keys_records_in_order_and_skips_none() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    w.create(
        "p",
        "python3.12",
        "index.handler",
        &[("index.py", PRINTER, 0o644)],
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
    let arn = described["StreamDescriptionSummary"]["StreamARN"]
        .as_str()
        .unwrap()
        .to_owned();
    // LATEST: what is already in a shard when the mapping starts is not due, but a child's records are.
    w.ok("CreateEventSourceMapping", json!({"FunctionName": "p", "EventSourceArn": arn, "StartingPosition": "LATEST", "BatchSize": 100})).await;
    let put = |n: u32| {
        w.peer(
            "kinesis",
            "PutRecord",
            json!({"StreamName": "s", "PartitionKey": "k", "Data": base64::encode(format!("r{n}").as_bytes())}),
        )
    };
    put(1).await;
    w.eventually("the first record", || async {
        !logged(&w, "p", "aws:kinesis").await.is_empty()
    })
    .await;
    for n in 2..=4 {
        put(n).await;
    }
    let listed = w
        .peer("kinesis", "ListShards", json!({"StreamName": "s"}))
        .await;
    let parent = listed["Shards"][0]["ShardId"].as_str().unwrap().to_owned();
    w.peer(
        "kinesis",
        "SplitShard",
        json!({"StreamName": "s", "ShardToSplit": parent, "NewStartingHashKey": (1_u128 << 127).to_string()}),
    )
    .await;
    // Written to a child at once, before the poller has listed the children.
    for n in 5..=7 {
        put(n).await;
    }
    w.eventually("all seven records", || async {
        logged(&w, "p", "aws:kinesis").await.len() >= 7
    })
    .await;
    let got = logged(&w, "p", "aws:kinesis").await;
    let want: Vec<String> = (1..=7).map(|n| format!("GOT aws:kinesis r{n}")).collect();
    assert_eq!(got, want);
    w.stop().await;
}
