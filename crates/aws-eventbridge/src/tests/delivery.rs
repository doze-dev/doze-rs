//! Delivery to targets through the peer bus: queues, topics, streams, log groups, other buses; input settings; and what a failure does.

use aws_peers::Mode;
use serde_json::{Value, json};

use super::{ACCOUNT, World, allow_events, caller};
use crate::deliver::{Delivery, Failure};
use crate::event::Event;
use crate::model::StoredTarget;

/// A rule on a bus, a queue it may deliver to, and the target; returns the rule's ARN.
fn wired(w: &mut World, bus: &str, pattern: &Value, extra: &Value) -> String {
    let rule = w.rule(bus, "r", pattern);
    let queue = w.queue("q", &rule);
    let mut target = json!({"Id": "t", "Arn": queue});
    for (k, v) in extra.as_object().unwrap() {
        target[k] = v.clone();
    }
    w.ok(
        "PutTargets",
        &json!({"Rule": "r", "EventBusName": bus, "Targets": [target]}),
    );
    rule
}

#[test]
fn a_matching_event_reaches_the_queue_as_the_event_aws_recorded() {
    let mut w = World::new();
    w.bus("shop");
    wired(
        &mut w,
        "shop",
        &json!({"source": ["shop.orders"], "detail": {"total": [{"numeric": [">", 100]}]}}),
        &json!({}),
    );
    let put = |w: &mut World, total: i64| {
        w.ok("PutEvents", &json!({"Entries": [{"EventBusName": "shop", "Source": "shop.orders", "DetailType": "placed",
            "Detail": json!({"total": total}).to_string(), "Resources": ["arn:aws:x:::order/1"]}]}))
    };
    let first = put(&mut w, 50);
    assert_eq!(first["FailedEntryCount"], 0);
    let second = put(&mut w, 250);
    // Only the second matches the numeric filter.
    let got = w.bodies("q");
    assert_eq!(got.len(), 1);
    let e = &got[0];
    assert_eq!(e["version"], "0");
    assert_eq!(e["id"], second["Entries"][0]["EventId"]);
    assert_eq!(e["detail"], json!({"total": 250}));
    assert_eq!(e["detail-type"], "placed");
    assert_eq!(e["source"], "shop.orders");
    assert_eq!(e["account"], ACCOUNT);
    assert_eq!(e["region"], "us-east-1");
    assert_eq!(e["resources"], json!(["arn:aws:x:::order/1"]));
    assert!(e["time"].as_str().unwrap().ends_with('Z'));
    assert_ne!(
        first["Entries"][0]["EventId"],
        second["Entries"][0]["EventId"]
    );
}

#[test]
fn a_disabled_rule_and_another_bus_get_nothing() {
    let mut w = World::new();
    w.bus("shop");
    w.bus("other");
    wired(&mut w, "shop", &json!({"source": ["x"]}), &json!({}));
    w.ok("DisableRule", &json!({"Name": "r", "EventBusName": "shop"}));
    w.put("shop", "x", &json!({}));
    assert!(w.drain("q").is_empty());
    w.ok("EnableRule", &json!({"Name": "r", "EventBusName": "shop"}));
    w.put("other", "x", &json!({}));
    assert!(w.drain("q").is_empty());
    w.put("shop", "x", &json!({}));
    assert_eq!(w.drain("q").len(), 1);
}

#[test]
fn input_settings_shape_what_the_queue_gets() {
    let mut w = World::new();
    w.bus("shop");
    wired(
        &mut w,
        "shop",
        &json!({"source": ["x"]}),
        &json!({
            "InputTransformer": {
                "InputPathsMap": {"who": "$.detail.customer", "kind": "$.detail-type"},
                "InputTemplate": "{\"message\": \"<who> did <kind>\"}",
            }
        }),
    );
    w.put("shop", "x", &json!({"customer": "ada"}));
    assert_eq!(w.drain("q")[0]["Body"], "{\"message\": \"ada did x\"}");
    for (settings, want) in [
        (json!({"Input": "{\"fixed\": true}"}), "{\"fixed\": true}"),
        (json!({"InputPath": "$.detail"}), "{\"customer\":\"ada\"}"),
    ] {
        let mut target = json!({"Id": "t", "Arn": w.core.rules.get("000000000000/shop/r").unwrap().targets[0].arn});
        for (k, v) in settings.as_object().unwrap() {
            target[k] = v.clone();
        }
        w.ok(
            "PutTargets",
            &json!({"Rule": "r", "EventBusName": "shop", "Targets": [target]}),
        );
        w.put("shop", "x", &json!({"customer": "ada"}));
        assert_eq!(w.drain("q")[0]["Body"], want);
    }
}

#[test]
fn a_fifo_queue_gets_the_group_the_target_names() {
    let mut w = World::new();
    w.bus("shop");
    let rule = w.rule("shop", "r", &json!({"source": ["x"]}));
    let made = w
        .peer(
            "sqs",
            "CreateQueue",
            &json!({"QueueName": "q.fifo", "Attributes": {"FifoQueue": "true"}}),
        )
        .unwrap();
    let arn = w
        .peer(
            "sqs",
            "GetQueueAttributes",
            &json!({"QueueUrl": made["QueueUrl"], "AttributeNames": ["QueueArn"]}),
        )
        .unwrap()["Attributes"]["QueueArn"]
        .clone();
    w.peer("sqs", "SetQueueAttributes", &json!({"QueueUrl": made["QueueUrl"], "Attributes": {"Policy": allow_events(arn.as_str().unwrap(), &rule)}})).unwrap();
    w.ok("PutTargets", &json!({"Rule": "r", "EventBusName": "shop", "Targets": [{"Id": "t", "Arn": arn, "SqsParameters": {"MessageGroupId": "orders"}}]}));
    w.put("shop", "x", &json!({}));
    let got = w.drain("q.fifo");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0]["Attributes"]["MessageGroupId"], "orders");
}

#[test]
fn the_queue_policy_is_asked_and_a_queue_that_does_not_name_the_rule_gets_nothing_under_enforce() {
    let mut w = World::new();
    w.bus("shop");
    wired(&mut w, "shop", &json!({"source": ["x"]}), &json!({}));
    // Soft (the binary's default): a policy that would refuse is logged, and the event still goes.
    w.peer("sqs", "SetQueueAttributes", &json!({"QueueUrl": World::queue_url("q"), "Attributes": {"Policy": allow_events("arn:aws:sqs:us-east-1:000000000000:q", "arn:aws:events:us-east-1:000000000000:rule/shop/other")}})).unwrap();
    w.put("shop", "x", &json!({}));
    assert_eq!(w.drain("q").len(), 1);
    w.peers.set_mode(Mode::Enforce);
    w.put("shop", "x", &json!({}));
    assert!(w.drain("q").is_empty(), "the policy names another rule");
    // A target with a role is delivered as the role and the queue's policy is not asked.
    let arn = w.core.rules.get("000000000000/shop/r").unwrap().targets[0]
        .arn
        .clone();
    w.ok("PutTargets", &json!({"Rule": "r", "EventBusName": "shop", "Targets": [{"Id": "t", "Arn": arn, "RoleArn": "arn:aws:iam::000000000000:role/r"}]}));
    w.put("shop", "x", &json!({}));
    assert_eq!(w.drain("q").len(), 1);
}

#[test]
fn what_cannot_be_delivered_goes_to_the_dead_letter_queue_with_the_documented_attributes() {
    let mut w = World::new();
    w.bus("shop");
    let rule = w.rule("shop", "r", &json!({"source": ["x"]}));
    let dlq = w.queue("dlq", &rule);
    let gone = "arn:aws:sqs:us-east-1:000000000000:gone";
    w.ok("PutTargets", &json!({"Rule": "r", "EventBusName": "shop", "Targets": [{"Id": "t", "Arn": gone, "DeadLetterConfig": {"Arn": dlq}}]}));
    let put = w.put("shop", "x", &json!({"n": 1}));
    let got = w.drain("dlq");
    assert_eq!(got.len(), 1);
    let m = &got[0];
    assert_eq!(
        serde_json::from_str::<Value>(m["Body"].as_str().unwrap()).unwrap()["id"],
        put["Entries"][0]["EventId"]
    );
    let attr = |n: &str| {
        m["MessageAttributes"][n]["StringValue"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(attr("RULE_ARN"), rule);
    assert_eq!(attr("TARGET_ARN"), gone);
    assert_eq!(attr("EXHAUSTED_RETRY_CONDITION"), "MaximumRetryAttempts");
    assert!(!attr("ERROR_CODE").is_empty() && !attr("ERROR_MESSAGE").is_empty());
    assert_eq!(attr("RETRY_ATTEMPTS"), "0");
}

/// A delivery to the queue `q`, built by hand so a failure can be made to be one that may pass.
fn delivery(w: &mut World, retry_policy: &Value) -> Delivery {
    w.bus("shop");
    let rule = w.rule("shop", "r", &json!({"source": ["x"]}));
    let queue = w.queue("q", &rule);
    let spec = json!({"Id": "t", "Arn": queue, "RetryPolicy": retry_policy});
    Delivery {
        rule_arn: rule,
        rule_name: "r".into(),
        target: StoredTarget {
            id: "t".into(),
            arn: queue,
            json: spec.to_string(),
        },
        spec,
        payload: "{\"hello\":1}".into(),
        event: Event {
            id: "e1".into(),
            source: "x".into(),
            detail_type: "x".into(),
            account: ACCOUNT.into(),
            region: "us-east-1".into(),
            time_ms: super::START,
            resources: vec![],
            detail: json!({}),
            replay_name: None,
        },
    }
}

fn passing() -> Failure {
    Failure {
        code: "ThrottlingException".into(),
        message: "slow down".into(),
        retryable: true,
    }
}

#[test]
fn a_failure_that_may_pass_is_tried_again_after_a_wait_and_then_delivered() {
    let mut w = World::new();
    let d = delivery(
        &mut w,
        &json!({"MaximumRetryAttempts": 5, "MaximumEventAgeInSeconds": 3600}),
    );
    w.rt.block_on(w.core.failed(d, passing(), 1, &caller()));
    assert_eq!(w.core.retries.len(), 1);
    let due = w.core.next_due().unwrap();
    assert_eq!(
        due,
        super::START + 1000,
        "one second after the first failure"
    );
    // Not yet: nothing happens before the deadline.
    assert!(w.drain("q").is_empty());
    let next = w.wake_at(due);
    assert!(w.core.retries.is_empty());
    assert_eq!(next, None, "nothing is left to wait for");
    assert_eq!(w.drain("q")[0]["Body"], "{\"hello\":1}");
}

#[test]
fn the_wait_doubles_to_a_minute_and_retries_run_out_into_the_dead_letter_queue() {
    let mut w = World::new();
    let mut d = delivery(&mut w, &json!({"MaximumRetryAttempts": 2}));
    let dlq = w.queue("dlq", &d.rule_arn.clone());
    d.spec["DeadLetterConfig"] = json!({"Arn": dlq});
    w.rt.block_on(w.core.failed(d.clone(), passing(), 1, &caller()));
    assert_eq!(w.core.retries[0].due_ms - super::START, 1000);
    w.core.retries.clear();
    w.rt.block_on(w.core.failed(d.clone(), passing(), 2, &caller()));
    assert_eq!(w.core.retries[0].due_ms - super::START, 2000);
    w.core.retries.clear();
    // The third failure is the second retry: no more are allowed.
    w.rt.block_on(w.core.failed(d, passing(), 3, &caller()));
    assert!(w.core.retries.is_empty());
    let got = w.drain("dlq");
    assert_eq!(
        got[0]["MessageAttributes"]["RETRY_ATTEMPTS"]["StringValue"],
        "2"
    );
    assert_eq!(
        got[0]["MessageAttributes"]["ERROR_CODE"]["StringValue"],
        "ThrottlingException"
    );
}

#[test]
fn an_event_too_old_is_given_up_on() {
    let mut w = World::new();
    let mut d = delivery(&mut w, &json!({"MaximumEventAgeInSeconds": 60}));
    let dlq = w.queue("dlq", &d.rule_arn.clone());
    d.spec["DeadLetterConfig"] = json!({"Arn": dlq});
    w.clock.advance(61_000);
    w.rt.block_on(w.core.failed(d, passing(), 1, &caller()));
    assert!(w.core.retries.is_empty());
    assert_eq!(
        w.drain("dlq")[0]["MessageAttributes"]["EXHAUSTED_RETRY_CONDITION"]["StringValue"],
        "MaximumEventAge"
    );
}

#[test]
fn a_target_on_a_service_nothing_delivers_to_is_kept_and_skipped() {
    let mut w = World::new();
    w.bus("shop");
    w.rule("shop", "r", &json!({"source": ["x"]}));
    for arn in [
        "arn:aws:ecs:us-east-1:000000000000:cluster/c",
        "arn:aws:lambda:us-east-1:000000000000:function:f",
        "arn:aws:states:us-east-1:000000000000:stateMachine:m",
        "arn:aws:events:us-east-1:000000000000:api-destination/d/1",
    ] {
        w.ok(
            "PutTargets",
            &json!({"Rule": "r", "EventBusName": "shop", "Targets": [{"Id": "t", "Arn": arn}]}),
        );
        let put = w.put("shop", "x", &json!({}));
        assert_eq!(put["FailedEntryCount"], 0, "{arn}");
    }
    assert!(w.core.retries.is_empty());
}

#[test]
fn an_event_bus_target_puts_the_event_on_that_bus_and_a_loop_ends() {
    let mut w = World::new();
    w.bus("a");
    w.bus("b");
    let bus_b = "arn:aws:events:us-east-1:000000000000:event-bus/b";
    w.rule("a", "forward", &json!({"source": ["x"]}));
    w.ok(
        "PutTargets",
        &json!({"Rule": "forward", "EventBusName": "a", "Targets": [{"Id": "t", "Arn": bus_b}]}),
    );
    let rule = w.rule("b", "r", &json!({"source": ["x"]}));
    let queue = w.queue("q", &rule);
    w.ok(
        "PutTargets",
        &json!({"Rule": "r", "EventBusName": "b", "Targets": [{"Id": "t", "Arn": queue}]}),
    );
    let put = w.put("a", "x", &json!({"n": 1}));
    let got = w.bodies("q");
    assert_eq!(got.len(), 1);
    assert_eq!(
        got[0]["id"], put["Entries"][0]["EventId"],
        "the event keeps its id"
    );
    // A bus that targets itself does not run for ever.
    w.ok(
        "PutTargets",
        &json!({"Rule": "r", "EventBusName": "b", "Targets": [{"Id": "self", "Arn": bus_b}]}),
    );
    w.put("b", "x", &json!({}));
    assert!(!w.drain("q").is_empty());
}

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
    w.peer(
        "sns",
        "Subscribe",
        &json!({"TopicArn": topic, "Protocol": "sqs", "Endpoint": queue, "Attributes": attrs}),
    )
    .unwrap();
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
    let it = w.peer("kinesis", "GetShardIterator", &json!({"StreamName": "s", "ShardId": "shardId-000000000000", "ShardIteratorType": "TRIM_HORIZON"})).unwrap();
    let records = w
        .peer(
            "kinesis",
            "GetRecords",
            &json!({"ShardIterator": it["ShardIterator"]}),
        )
        .unwrap();
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
    w.peer(
        "logs",
        "CreateLogGroup",
        &json!({"logGroupName": "/events"}),
    )
    .unwrap();
    let group = "arn:aws:logs:us-east-1:000000000000:log-group:/events:*";
    shop_rule_with(&mut w, &json!([{"Id": "logs", "Arn": group}]));
    w.put("shop", "x", &json!({"k": "key-1"}));
    w.put("shop", "x", &json!({"k": "key-2"}));
    let found = w
        .peer(
            "logs",
            "FilterLogEvents",
            &json!({"logGroupName": "/events"}),
        )
        .unwrap();
    let events = found["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    let line: Value = serde_json::from_str(events[0]["message"].as_str().unwrap()).unwrap();
    assert_eq!(line["detail"], json!({"k": "key-1"}));
    assert_eq!(events[0]["logStreamName"], "r");
}
