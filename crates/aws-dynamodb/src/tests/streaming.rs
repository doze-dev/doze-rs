//! Item changes sent to a Kinesis streaming destination, in the record shape of the developer guide.

use serde_json::{Value, json};

use super::bus::Bus;

#[tokio::test]
async fn item_changes_reach_a_kinesis_streaming_destination() {
    let bus = Bus::new();
    bus.table("tab").await;
    bus.kinesis(
        "CreateStream",
        json!({"StreamName": "changes", "ShardCount": 1}),
    )
    .await;
    let stream = "arn:aws:kinesis:us-east-1:000000000000:stream/changes";
    let on = bus
        .ok(
            "EnableKinesisStreamingDestination",
            json!({"TableName": "tab", "StreamArn": stream}),
        )
        .await;
    assert_eq!(on["DestinationStatus"], "ENABLING");
    let d = bus
        .ok(
            "DescribeKinesisStreamingDestination",
            json!({"TableName": "tab"}),
        )
        .await;
    assert_eq!(
        d["KinesisDataStreamDestinations"][0]["DestinationStatus"], "ACTIVE",
        "{d}"
    );
    bus.ok(
        "PutItem",
        json!({"TableName": "tab", "Item": {"pk": {"S": "a"}, "v": {"N": "1"}}}),
    )
    .await;
    bus.ok(
        "PutItem",
        json!({"TableName": "tab", "Item": {"pk": {"S": "a"}, "v": {"N": "2"}}}),
    )
    .await;
    bus.ok(
        "DeleteItem",
        json!({"TableName": "tab", "Key": {"pk": {"S": "a"}}}),
    )
    .await;
    let shards = bus
        .kinesis("ListShards", json!({"StreamName": "changes"}))
        .await;
    let shard = shards["Shards"][0]["ShardId"].as_str().unwrap().to_owned();
    let it = bus
        .kinesis(
            "GetShardIterator",
            json!({"StreamName": "changes", "ShardId": shard, "ShardIteratorType": "TRIM_HORIZON"}),
        )
        .await;
    let got = bus
        .kinesis("GetRecords", json!({"ShardIterator": it["ShardIterator"]}))
        .await;
    check_records(&got);
    then_disabled(&bus, stream, &got).await;
}

/// The three records of an insert, a change and a delete, in the shape of the guide.
fn check_records(got: &Value) {
    let records: Vec<Value> = got["Records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            serde_json::from_slice(&aws_core::base64::decode(r["Data"].as_str().unwrap()).unwrap())
                .unwrap()
        })
        .collect();
    let events: Vec<&str> = records
        .iter()
        .map(|r| r["eventName"].as_str().unwrap())
        .collect();
    assert_eq!(events, ["INSERT", "MODIFY", "REMOVE"]);
    let modify = &records[1];
    assert_eq!(modify["tableName"], "tab");
    assert_eq!(modify["eventSource"], "aws:dynamodb");
    assert_eq!(modify["recordFormat"], "application/json");
    assert_eq!(modify["dynamodb"]["Keys"], json!({"pk": {"S": "a"}}));
    assert_eq!(modify["dynamodb"]["OldImage"]["v"], json!({"N": "1"}));
    assert_eq!(modify["dynamodb"]["NewImage"]["v"], json!({"N": "2"}));
    assert_eq!(
        modify["dynamodb"]["ApproximateCreationDateTimePrecision"],
        "MILLISECOND"
    );
}

/// Microseconds when asked; nothing once disabled; a stream that is not there fails to enable.
async fn then_disabled(bus: &Bus, stream: &str, got: &Value) {
    let up = bus
        .ok(
            "UpdateKinesisStreamingDestination",
            json!({"TableName": "tab", "StreamArn": stream, "UpdateKinesisStreamingConfiguration": {"ApproximateCreationDateTimePrecision": "MICROSECOND"}}),
        )
        .await;
    assert_eq!(up["DestinationStatus"], "UPDATING");
    let off = bus
        .ok(
            "DisableKinesisStreamingDestination",
            json!({"TableName": "tab", "StreamArn": stream}),
        )
        .await;
    assert_eq!(off["DestinationStatus"], "DISABLING");
    bus.ok(
        "PutItem",
        json!({"TableName": "tab", "Item": {"pk": {"S": "b"}}}),
    )
    .await;
    let later = bus
        .kinesis(
            "GetRecords",
            json!({"ShardIterator": got["NextShardIterator"]}),
        )
        .await;
    assert_eq!(later["Records"], json!([]));
    let d = bus
        .ok(
            "DescribeKinesisStreamingDestination",
            json!({"TableName": "tab"}),
        )
        .await;
    assert_eq!(
        d["KinesisDataStreamDestinations"][0]["DestinationStatus"],
        "DISABLED"
    );
    // A stream that is not there fails to enable, and says why.
    let missing = "arn:aws:kinesis:us-east-1:000000000000:stream/none";
    bus.ok(
        "EnableKinesisStreamingDestination",
        json!({"TableName": "tab", "StreamArn": missing}),
    )
    .await;
    let d = bus
        .ok(
            "DescribeKinesisStreamingDestination",
            json!({"TableName": "tab"}),
        )
        .await;
    let failed = d["KinesisDataStreamDestinations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["StreamArn"] == missing)
        .unwrap()
        .clone();
    assert_eq!(failed["DestinationStatus"], "ENABLE_FAILED");
    assert!(failed.get("DestinationStatusDescription").is_some());
}
