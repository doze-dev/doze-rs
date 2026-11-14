//! The four operations over a real DynamoDB: list, describe, an iterator of each kind, and records read in pages.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines
)]

use std::sync::Arc;
use std::time::Duration;

use aws_testkit::{Harness, TempDir};
use doze_runtime::{Ctx, ManualClock, Tasks};
use serde_json::{Value, json};

use super::*;

const NOW_MS: i64 = 1_700_000_000_000;

struct World {
    ddb: Arc<Dynamodb>,
    streams: Streams,
    /// Decoders: a harness holds a service of its own.
    ddb_h: Harness<Dynamodb>,
    streams_h: Harness<Streams>,
    clock: Arc<ManualClock>,
    _dirs: [TempDir; 2],
}

fn dynamodb(dir: &TempDir, clock: &Arc<ManualClock>) -> Dynamodb {
    Dynamodb::new(Arc::new(Ctx {
        dir: dir.path().to_path_buf(),
        idle: Duration::from_secs(3600),
        tasks: Tasks::new(),
        clock: clock.clone(),
        ext: None,
    }))
}

impl World {
    fn new() -> Self {
        let (dir, spare) = (
            TempDir::new("ddb-streams"),
            TempDir::new("ddb-streams-decode"),
        );
        let clock = Arc::new(ManualClock::at(NOW_MS));
        let ddb = Arc::new(dynamodb(&dir, &clock));
        let decoder = Arc::new(dynamodb(&spare, &clock));
        Self {
            streams: Streams::new(Arc::clone(&ddb), None),
            streams_h: Harness::new(Streams::new(decoder, None)),
            ddb_h: Harness::new(dynamodb(&spare, &clock)),
            ddb,
            clock,
            _dirs: [dir, spare],
        }
    }

    async fn ddb(&self, op: &str, body: Value) -> Value {
        let ix = self.ddb_h.op(op);
        let call = self.ddb_h.call(ix, &body).unwrap();
        let out = self.ddb.call(call).await.unwrap();
        self.ddb_h.json(ix, &out)
    }

    async fn call(&self, op: &str, body: Value) -> Result<Value, ApiError> {
        let ix = self.streams_h.op(op);
        let call = self.streams_h.call(ix, &body)?;
        let out = self.streams.call(call).await?;
        Ok(self.streams_h.json(ix, &out))
    }

    async fn ok(&self, op: &str, body: Value) -> Value {
        self.call(op, body)
            .await
            .unwrap_or_else(|e| panic!("{op}: {} {}", e.code, e.message))
    }
}

async fn orders(w: &World) -> String {
    let made = w
        .ddb(
            "CreateTable",
            json!({"TableName": "orders", "BillingMode": "PAY_PER_REQUEST",
                "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}, {"AttributeName": "sk", "KeyType": "RANGE"}],
                "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}, {"AttributeName": "sk", "AttributeType": "N"}],
                "StreamSpecification": {"StreamEnabled": true, "StreamViewType": "NEW_AND_OLD_IMAGES"}}),
        )
        .await;
    made["TableDescription"]["LatestStreamArn"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test(flavor = "current_thread")]
async fn a_stream_is_listed_and_described_with_its_one_shard() {
    let w = World::new();
    let arn = orders(&w).await;
    w.ddb(
        "CreateTable",
        json!({"TableName": "plain", "BillingMode": "PAY_PER_REQUEST",
            "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
            "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}]}),
    )
    .await;
    let listed = w.ok("ListStreams", json!({})).await;
    assert_eq!(
        listed,
        json!({"Streams": [{"StreamArn": arn, "TableName": "orders", "StreamLabel": "2023-11-14T22:13:20.000"}]})
    );
    assert_eq!(
        w.ok("ListStreams", json!({"TableName": "plain"})).await,
        json!({"Streams": []})
    );

    let d = w.ok("DescribeStream", json!({"StreamArn": arn})).await["StreamDescription"].clone();
    assert_eq!(d["StreamStatus"], "ENABLED");
    assert_eq!(d["StreamViewType"], "NEW_AND_OLD_IMAGES");
    assert_eq!(d["TableName"], "orders");
    assert_eq!(d["CreationRequestDateTime"].as_f64(), Some(1_700_000_000.0));
    assert_eq!(
        d["KeySchema"],
        json!([{"AttributeName": "pk", "KeyType": "HASH"}, {"AttributeName": "sk", "KeyType": "RANGE"}])
    );
    let shard = d["Shards"][0].clone();
    assert_eq!(
        shard["SequenceNumberRange"],
        json!({"StartingSequenceNumber": "000000000000000000001"})
    );
    let shard_id = shard["ShardId"].as_str().unwrap();
    assert!(shard_id.starts_with("shardId-00000001700000000000-"));
    let past = w
        .ok(
            "DescribeStream",
            json!({"StreamArn": arn, "ExclusiveStartShardId": shard_id}),
        )
        .await;
    assert_eq!(past["StreamDescription"]["Shards"], json!([]));

    let gone = format!("{}x", &arn[..arn.len() - 1]);
    let e = w
        .call("DescribeStream", json!({"StreamArn": gone}))
        .await
        .unwrap_err();
    assert_eq!(e.code, "ResourceNotFoundException");
}

#[tokio::test(flavor = "current_thread")]
async fn records_are_read_from_each_kind_of_iterator_in_pages() {
    let w = World::new();
    let arn = orders(&w).await;
    let d = w.ok("DescribeStream", json!({"StreamArn": arn})).await;
    let shard = d["StreamDescription"]["Shards"][0]["ShardId"].clone();
    for sk in 1..=3 {
        w.ddb(
            "PutItem",
            json!({"TableName": "orders", "Item": {"pk": {"S": "a"}, "sk": {"N": sk.to_string()}}}),
        )
        .await;
    }
    w.clock.advance(5_000);
    w.ddb(
        "DeleteItem",
        json!({"TableName": "orders", "Key": {"pk": {"S": "a"}, "sk": {"N": "1"}}}),
    )
    .await;

    let it = |kind: &str, extra: Value| {
        let mut body = json!({"StreamArn": arn, "ShardId": shard, "ShardIteratorType": kind});
        if let Some(m) = extra.as_object() {
            for (k, v) in m {
                body[k] = v.clone();
            }
        }
        body
    };
    let horizon = w
        .ok("GetShardIterator", it("TRIM_HORIZON", json!({})))
        .await["ShardIterator"]
        .clone();
    let page = w
        .ok("GetRecords", json!({"ShardIterator": horizon, "Limit": 2}))
        .await;
    let records = page["Records"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["eventName"], "INSERT");
    assert_eq!(records[0]["eventSource"], "aws:dynamodb");
    assert_eq!(
        records[0]["dynamodb"]["SequenceNumber"],
        "000000000000000000001"
    );
    assert_eq!(
        records[0]["dynamodb"]["Keys"],
        json!({"pk": {"S": "a"}, "sk": {"N": "1"}})
    );
    let rest = w
        .ok(
            "GetRecords",
            json!({"ShardIterator": page["NextShardIterator"]}),
        )
        .await;
    let names: Vec<&str> = rest["Records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["eventName"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["INSERT", "REMOVE"]);
    let empty = w
        .ok(
            "GetRecords",
            json!({"ShardIterator": rest["NextShardIterator"]}),
        )
        .await;
    assert_eq!(empty["Records"], json!([]));
    assert!(
        empty["NextShardIterator"].is_string(),
        "an open shard always has a next iterator"
    );

    let latest = w.ok("GetShardIterator", it("LATEST", json!({}))).await["ShardIterator"].clone();
    assert_eq!(
        w.ok("GetRecords", json!({"ShardIterator": latest})).await["Records"],
        json!([])
    );
    let at = w
        .ok(
            "GetShardIterator",
            it(
                "AT_SEQUENCE_NUMBER",
                json!({"SequenceNumber": "000000000000000000003"}),
            ),
        )
        .await["ShardIterator"]
        .clone();
    let from3 = w.ok("GetRecords", json!({"ShardIterator": at})).await;
    assert_eq!(from3["Records"].as_array().unwrap().len(), 2);
    let after = w
        .ok(
            "GetShardIterator",
            it(
                "AFTER_SEQUENCE_NUMBER",
                json!({"SequenceNumber": "000000000000000000003"}),
            ),
        )
        .await["ShardIterator"]
        .clone();
    let after3 = w.ok("GetRecords", json!({"ShardIterator": after})).await;
    assert_eq!(after3["Records"][0]["eventName"], "REMOVE");
    assert_eq!(
        after3["Records"][0]["dynamodb"]["OldImage"],
        json!({"pk": {"S": "a"}, "sk": {"N": "1"}})
    );
    let stamp = w
        .ok(
            "GetShardIterator",
            it("AT_TIMESTAMP", json!({"Timestamp": 1_700_000_001.0})),
        )
        .await["ShardIterator"]
        .clone();
    let since = w.ok("GetRecords", json!({"ShardIterator": stamp})).await;
    assert_eq!(since["Records"].as_array().unwrap().len(), 1);

    let e = w
        .call("GetShardIterator", it("AT_SEQUENCE_NUMBER", json!({})))
        .await
        .unwrap_err();
    assert_eq!(e.code, "ValidationException");
    let mut wrong = it("TRIM_HORIZON", json!({}));
    wrong["ShardId"] = json!("shardId-00000000000000000000-00000000");
    let e = w.call("GetShardIterator", wrong).await.unwrap_err();
    assert_eq!(e.code, "ResourceNotFoundException");
    let e = w
        .call("GetRecords", json!({"ShardIterator": "not an iterator"}))
        .await
        .unwrap_err();
    assert_eq!(e.code, "ValidationException");
    let e = w
        .call(
            "GetRecords",
            json!({"ShardIterator": horizon, "Limit": 1001}),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "ValidationException");

    w.clock.advance(15 * 60 * 1000 + 1);
    let e = w
        .call("GetRecords", json!({"ShardIterator": horizon}))
        .await
        .unwrap_err();
    assert_eq!(e.code, "ExpiredIteratorException");
}

#[tokio::test(flavor = "current_thread")]
async fn a_time_to_live_removal_names_the_service_as_aws_capitalises_it() {
    let event = json!({"eventName": "REMOVE", "userIdentity": {"type": "Service", "principalId": "dynamodb.amazonaws.com"}});
    assert_eq!(
        wire_record(event)["userIdentity"],
        json!({"Type": "Service", "PrincipalId": "dynamodb.amazonaws.com"})
    );
}
