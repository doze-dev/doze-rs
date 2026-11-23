//! A Kinesis stream's page of its own: its settings (each its own call), the Data Viewer's reads, the sharing policy, and the delete that takes its
//! consumers with it. Against real services on a real bus.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use serde_json::{Value, json};
use world::{field, world};

const BASE: &str = "/api/v1/resources";

async fn op(w: &world::World, path: &str, operation: &str, body: &Value) -> (u16, Value) {
    w.send("POST", &format!("{path}/op/{operation}"), body)
        .await
}

#[tokio::test]
async fn a_stream_is_made_with_its_tags_and_each_setting_is_its_own_call() {
    let w = world("lk-stream", Mode::Soft, &[]);
    let (status, made) = w
        .send(
            "POST",
            &format!("{BASE}/kinesis/streams/op/CreateStream"),
            &json!({"StreamName": "clickstream", "StreamModeDetails": {"StreamMode": "PROVISIONED"}, "ShardCount": 2,
                "Tags": {"stack": "larder", "env": "dev"}}),
        )
        .await;
    assert_eq!(status, 200, "{made}");
    let one = format!("{BASE}/kinesis/streams/clickstream");
    let (status, d) = w.get(&one).await;
    assert_eq!(status, 200, "{d}");
    assert_eq!(d["badges"][0]["text"], "Active");
    assert_eq!(d["badges"][0]["tone"], "good");
    assert_eq!(field(&d, "Open shards")["value"], "2");
    assert_eq!(d["tags"], true);
    let (_, tags) = w.get(&format!("{one}/tags")).await;
    assert_eq!(tags["tags"].as_array().unwrap().len(), 2);
    // The list's columns are the board's; the consumers are counted for its sentence.
    let (_, list) = w.get(&format!("{BASE}/kinesis/streams")).await;
    let titles: Vec<&str> = list["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Status", "Capacity mode", "Created", "Consumers"]);

    let (status, r) = op(
        &w,
        &one,
        "UpdateShardCount",
        &json!({"TargetShardCount": 4, "ScalingType": "UNIFORM_SCALING"}),
    )
    .await;
    assert_eq!(status, 200, "{r}");
    let (status, r) = op(
        &w,
        &one,
        "IncreaseStreamRetentionPeriod",
        &json!({"RetentionPeriodHours": 72}),
    )
    .await;
    assert_eq!(status, 200, "{r}");
    let (status, r) = op(
        &w,
        &one,
        "StartStreamEncryption",
        &json!({"EncryptionType": "KMS", "KeyId": "alias/larder-app"}),
    )
    .await;
    assert_eq!(status, 200, "{r}");
    let (status, r) = op(
        &w,
        &one,
        "EnableEnhancedMonitoring",
        &json!({"ShardLevelMetrics": ["IncomingBytes"]}),
    )
    .await;
    assert_eq!(status, 200, "{r}");
    let (_, d) = w.get(&one).await;
    // A list of names arrives as JSON text, which the page reads.
    assert_eq!(
        field(&d, "Shard-level metrics")["value"],
        "[\"IncomingBytes\"]"
    );
    assert_eq!(field(&d, "Open shards")["value"], "4");
    assert_eq!(field(&d, "Retention (hours)")["value"], "72");
    assert_eq!(field(&d, "Encryption")["value"], "KMS");
    let (status, r) = op(
        &w,
        &one,
        "DecreaseStreamRetentionPeriod",
        &json!({"RetentionPeriodHours": 24}),
    )
    .await;
    assert_eq!(status, 200, "{r}");
    // A decrease that is an increase is the service's refusal.
    let (status, r) = op(
        &w,
        &one,
        "DecreaseStreamRetentionPeriod",
        &json!({"RetentionPeriodHours": 48}),
    )
    .await;
    assert_eq!(status, 400, "{r}");
    let (status, r) = op(
        &w,
        &one,
        "StopStreamEncryption",
        &json!({"EncryptionType": "KMS", "KeyId": "alias/larder-app"}),
    )
    .await;
    assert_eq!(status, 200, "{r}");
    let (status, r) = op(
        &w,
        &one,
        "UpdateStreamMode",
        &json!({"StreamModeDetails": {"StreamMode": "ON_DEMAND"}}),
    )
    .await;
    assert_eq!(status, 200, "{r}");
    let (_, d) = w.get(&one).await;
    assert_eq!(field(&d, "Capacity mode")["value"], "ON_DEMAND");
    // The encryption edit's keys: KMS's aliases, read through the kind.
    let (status, aliases) = w
        .send(
            "POST",
            &format!("{BASE}/kinesis/streams/op/ListAliases"),
            &json!({}),
        )
        .await;
    assert_eq!(status, 200, "{aliases}");
    // A call cannot be pointed at another stream: the one in the address is the one acted on.
    w.app(
        "kinesis",
        "CreateStream",
        &json!({"StreamName": "other", "ShardCount": 1}),
    )
    .await;
    op(
        &w,
        &one,
        "IncreaseStreamRetentionPeriod",
        &json!({"StreamName": "other", "RetentionPeriodHours": 96}),
    )
    .await;
    let (_, other) = w.get(&format!("{BASE}/kinesis/streams/other")).await;
    assert_eq!(field(&other, "Retention (hours)")["value"], "24");
    let (_, mine) = w.get(&one).await;
    assert_eq!(field(&mine, "Retention (hours)")["value"], "96");
}

#[tokio::test]
async fn a_streams_records_are_read_through_an_iterator_and_its_sharing_policy_is_kept() {
    let w = world("lk-viewer", Mode::Soft, &[]);
    w.app(
        "kinesis",
        "CreateStream",
        &json!({"StreamName": "clickstream", "ShardCount": 1}),
    )
    .await;
    for (i, key) in ["s-0a3d", "s-91b7"].iter().enumerate() {
        w.app(
            "kinesis",
            "PutRecord",
            &json!({"StreamName": "clickstream", "PartitionKey": key, "Data": format!("e{i}")}),
        )
        .await;
    }
    let one = format!("{BASE}/kinesis/streams/clickstream");
    let (status, shards) = op(&w, &one, "ListShards", &json!({})).await;
    assert_eq!(status, 200, "{shards}");
    let shard = shards["Shards"][0]["ShardId"].as_str().unwrap().to_owned();
    let (status, it) = op(
        &w,
        &one,
        "GetShardIterator",
        &json!({"ShardId": shard, "ShardIteratorType": "TRIM_HORIZON"}),
    )
    .await;
    assert_eq!(status, 200, "{it}");
    let (status, got) = op(
        &w,
        &one,
        "GetRecords",
        &json!({"ShardIterator": it["ShardIterator"], "Limit": 50}),
    )
    .await;
    assert_eq!(status, 200, "{got}");
    assert_eq!(got["Records"].as_array().unwrap().len(), 2);
    assert!(got["NextShardIterator"].is_string());

    // The sharing policy: none (the service answers an empty one), put, read back, deleted.
    let (status, none) = op(&w, &one, "GetResourcePolicy", &json!({})).await;
    assert_eq!(status, 200, "{none}");
    assert_eq!(none["Policy"], "{}");
    let policy = json!({"Version": "2012-10-17", "Statement": [{"Sid": "read", "Effect": "Allow",
        "Principal": {"AWS": "arn:aws:iam::000000000000:role/report-job"},
        "Action": ["kinesis:GetRecords"], "Resource": "arn:aws:kinesis:us-east-1:000000000000:stream/clickstream"}]});
    let (status, put) = op(
        &w,
        &one,
        "PutResourcePolicy",
        &json!({"Policy": policy.to_string()}),
    )
    .await;
    assert_eq!(status, 200, "{put}");
    let (_, back) = op(&w, &one, "GetResourcePolicy", &json!({})).await;
    assert_eq!(
        serde_json::from_str::<Value>(back["Policy"].as_str().unwrap()).unwrap(),
        policy
    );
    let (status, gone) = op(&w, &one, "DeleteResourcePolicy", &json!({})).await;
    assert_eq!(status, 200, "{gone}");
}

#[tokio::test]
async fn a_stream_with_a_consumer_is_deleted_with_it_and_the_consumers_are_listed_first() {
    let w = world("lk-delete", Mode::Soft, &[]);
    let made = w
        .app(
            "kinesis",
            "CreateStream",
            &json!({"StreamName": "clickstream", "ShardCount": 1}),
        )
        .await;
    drop(made);
    let (_, d) = w.get(&format!("{BASE}/kinesis/streams/clickstream")).await;
    let arn = field(&d, "ARN")["value"].as_str().unwrap().to_owned();
    w.app(
        "kinesis",
        "RegisterStreamConsumer",
        &json!({"StreamARN": arn, "ConsumerName": "search-indexer"}),
    )
    .await;
    let (status, consumers) = w
        .send(
            "POST",
            &format!("{BASE}/kinesis/streams/op/ListStreamConsumers"),
            &json!({"StreamARN": arn}),
        )
        .await;
    assert_eq!(status, 200, "{consumers}");
    assert_eq!(consumers["Consumers"][0]["ConsumerName"], "search-indexer");
    let (_, list) = w.get(&format!("{BASE}/kinesis/streams")).await;
    assert_eq!(list["rows"][0]["cells"][3], "1");
    // Deleting takes the consumer with the stream, as the dialog says.
    w.delete(&format!("{BASE}/kinesis/streams"), "clickstream")
        .await;
}
