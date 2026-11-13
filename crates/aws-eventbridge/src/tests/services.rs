//! Delivery to a topic, a stream and a log group, through the services that run beside EventBridge on the peer bus.

use serde_json::{Value, json};

use super::World;

/// A rule on bus `shop` matching source `x`, with `targets` put on it; the clock is the wall clock (Logs refuses events older than its
/// own clock allows). Returns the rule's ARN.
fn shop_rule_with(w: &mut World, targets: &Value) -> String {
    w.bus("shop");
    let rule = w.rule("shop", "r", &json!({"source": ["x"]}));
    w.ok(
        "PutTargets",
        &json!({"Rule": "r", "EventBusName": "shop", "Targets": targets}),
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    w.clock.set(i64::try_from(now).unwrap());
    rule
}

#[test]
fn a_topic_is_published_to_with_the_shaped_input() {
    let mut w = World::new();
    let topic = w.peer("sns", "CreateTopic", &json!({"Name": "t"})).unwrap()["TopicArn"]
        .as_str()
        .unwrap()
        .to_owned();
    let rule = shop_rule_with(
        &mut w,
        &json!([{"Id": "topic", "Arn": topic, "InputPath": "$.detail"}]),
    );
    let queue = w.queue("tq", &rule);
    let attrs = json!({"RawMessageDelivery": "true"});
    let subscribe =
        json!({"TopicArn": topic, "Protocol": "sqs", "Endpoint": queue, "Attributes": attrs});
    w.peer("sns", "Subscribe", &subscribe).unwrap();
    w.put("shop", "x", &json!({"k": "key-1"}));
    assert_eq!(w.drain("tq")[0]["Body"], "{\"k\":\"key-1\"}");
}

#[test]
fn a_stream_gets_the_event_under_the_partition_key_the_target_names_or_the_event_id() {
    let mut w = World::new();
    let made = json!({"StreamName": "s", "ShardCount": 1});
    w.peer("kinesis", "CreateStream", &made).unwrap();
    let summary = w
        .peer(
            "kinesis",
            "DescribeStreamSummary",
            &json!({"StreamName": "s"}),
        )
        .unwrap();
    let stream = summary["StreamDescriptionSummary"]["StreamARN"].clone();
    shop_rule_with(
        &mut w,
        &json!([{"Id": "a", "Arn": stream, "KinesisParameters": {"PartitionKeyPath": "$.detail.k"}}]),
    );
    let put = w.put("shop", "x", &json!({"k": "key-1"}));
    // Without a path the partition key is the event's id (documented).
    w.ok(
        "PutTargets",
        &json!({"Rule": "r", "EventBusName": "shop", "Targets": [{"Id": "a", "Arn": stream}]}),
    );
    let second = w.put("shop", "x", &json!({"k": "key-2"}));
    let iterator = json!({"StreamName": "s", "ShardId": "shardId-000000000000", "ShardIteratorType": "TRIM_HORIZON"});
    let it = w.peer("kinesis", "GetShardIterator", &iterator).unwrap();
    let read = json!({"ShardIterator": it["ShardIterator"]});
    let records = w.peer("kinesis", "GetRecords", &read).unwrap();
    let keys: Vec<&Value> = records["Records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| &r["PartitionKey"])
        .collect();
    assert_eq!(keys, [&json!("key-1"), &second["Entries"][0]["EventId"]]);
    let data = aws_core::base64::decode(records["Records"][0]["Data"].as_str().unwrap()).unwrap();
    let event: Value = serde_json::from_slice(&data).unwrap();
    assert_eq!(event["id"], put["Entries"][0]["EventId"]);
}

#[test]
fn a_log_group_gets_the_event_as_a_line_in_a_stream_named_for_the_rule() {
    let mut w = World::new();
    let group_name = json!({"logGroupName": "/events"});
    w.peer("logs", "CreateLogGroup", &group_name).unwrap();
    let group = "arn:aws:logs:us-east-1:000000000000:log-group:/events:*";
    shop_rule_with(&mut w, &json!([{"Id": "logs", "Arn": group}]));
    w.put("shop", "x", &json!({"k": "key-1"}));
    w.put("shop", "x", &json!({"k": "key-2"}));
    let found = w.peer("logs", "FilterLogEvents", &group_name).unwrap();
    let events = found["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    let line: Value = serde_json::from_str(events[0]["message"].as_str().unwrap()).unwrap();
    assert_eq!(line["detail"], json!({"k": "key-1"}));
    assert_eq!(events[0]["logStreamName"], "r");
}
