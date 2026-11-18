//! Everything the service keeps is on disk: after the database is closed and opened again every read answers as it did.

use std::sync::Arc;

use aws_testkit::TempDir;
use doze_runtime::{ManualClock, Tasks};
use serde_json::{Value, json};

use super::{START, World};
use crate::core::Core;

const READS: [(&str, &str); 9] = [
    ("ListEventBuses", "{}"),
    ("DescribeEventBus", r#"{"Name": "shop"}"#),
    ("ListRules", r#"{"EventBusName": "shop"}"#),
    ("DescribeRule", r#"{"Name": "r", "EventBusName": "shop"}"#),
    (
        "ListTargetsByRule",
        r#"{"Rule": "r", "EventBusName": "shop"}"#,
    ),
    ("ListArchives", "{}"),
    ("ListReplays", "{}"),
    ("DescribeConnection", r#"{"Name": "c"}"#),
    ("DescribeApiDestination", r#"{"Name": "d"}"#),
];

/// Everything the service holds, read through its own operations.
fn everything(w: &mut World) -> Vec<Value> {
    let mut out: Vec<Value> = READS
        .iter()
        .map(|(op, req)| w.ok(op, &serde_json::from_str::<Value>(req).unwrap()))
        .collect();
    let rule = "arn:aws:events:us-east-1:000000000000:rule/shop/r";
    out.push(w.ok("ListTagsForResource", &json!({"ResourceARN": rule})));
    out
}

#[test]
fn a_reopened_database_answers_as_the_one_that_was_closed() {
    let dir = TempDir::new("eventbridge-persist");
    let mut w = World::new();
    // One clock for every opening, and the world's, so a replay is driven by the time the world moves.
    let clock = Arc::new(ManualClock::at(START));
    w.clock = clock.clone();
    let open =
        |dir: &TempDir| Core::open_in(dir.path(), clock.clone(), None, Tasks::new()).unwrap();
    w.core = open(&dir);
    w.bus("shop");
    w.rule("shop", "r", &json!({"source": ["x"]}));
    let arn = "arn:aws:sqs:us-east-1:000000000000:q";
    let target = json!({"Id": "t", "Arn": arn, "InputPath": "$.detail", "RetryPolicy": {"MaximumRetryAttempts": 3}});
    w.ok(
        "PutTargets",
        &json!({"Rule": "r", "EventBusName": "shop", "Targets": [target]}),
    );
    let rule_arn = "arn:aws:events:us-east-1:000000000000:rule/shop/r";
    w.ok(
        "TagResource",
        &json!({"ResourceARN": rule_arn, "Tags": [{"Key": "k", "Value": "v"}]}),
    );
    w.ok("PutPermission", &json!({"EventBusName": "shop", "StatementId": "s", "Action": "events:PutEvents", "Principal": "111122223333"}));
    let bus = "arn:aws:events:us-east-1:000000000000:event-bus/shop";
    w.ok(
        "CreateArchive",
        &json!({"ArchiveName": "a", "EventSourceArn": bus}),
    );
    w.put("shop", "x", &json!({"n": 1}));
    let s = START / 1000;
    let archive = "arn:aws:events:us-east-1:000000000000:archive/a";
    w.ok("StartReplay", &json!({"ReplayName": "rp", "EventSourceArn": archive, "EventStartTime": s - 60, "EventEndTime": s + 60, "Destination": {"Arn": bus}}));
    let basic = json!({"BasicAuthParameters": {"Username": "u", "Password": "p"}});
    let conn = w.ok(
        "CreateConnection",
        &json!({"Name": "c", "AuthorizationType": "BASIC", "AuthParameters": basic}),
    );
    w.ok("CreateApiDestination", &json!({"Name": "d", "ConnectionArn": conn["ConnectionArn"], "InvocationEndpoint": "http://localhost/x", "HttpMethod": "POST"}));
    let before = everything(&mut w);
    assert_eq!(before[5]["Archives"][0]["EventCount"], 1);
    w.core.db.take().unwrap().close();

    w.core = open(&dir);
    assert_eq!(everything(&mut w), before);
    // The archived event is still there to be replayed.
    let rows = w
        .core
        .archived
        .scan(w.core.db.as_ref(), "0", "z", 10, false);
    assert_eq!(rows.unwrap().len(), 1);
    w.ok("StartReplay", &json!({"ReplayName": "again", "EventSourceArn": archive, "EventStartTime": s - 60, "EventEndTime": s + 60, "Destination": {"Arn": bus}}));
    // The replay started before the stop was still going, and runs to its end after it.
    w.finish_replays();
    for name in ["rp", "again"] {
        assert_eq!(
            w.ok("DescribeReplay", &json!({"ReplayName": name}))["State"],
            "COMPLETED"
        );
    }
}

#[test]
fn an_accepted_event_and_a_waiting_retry_outlive_a_stop_and_are_delivered_after_it() {
    let dir = TempDir::new("eventbridge-pending");
    let mut w = World::new();
    let open = |dir: &TempDir, w: &World| {
        Core::open_in(
            dir.path(),
            w.clock.clone(),
            Some(w.peers.clone()),
            Tasks::new(),
        )
        .unwrap()
    };
    w.core = open(&dir, &w);
    w.bus("shop");
    let rule = w.rule("shop", "r", &json!({"source": ["x"]}));
    let q = w.queue("q", &rule);
    w.ok(
        "PutTargets",
        &json!({"Rule": "r", "EventBusName": "shop", "Targets": [{"Id": "t", "Arn": q}]}),
    );
    w.ok("PutEvents", &json!({"Entries": [{"EventBusName": "shop", "Source": "x", "DetailType": "x", "Detail": "{\"n\":1}"}]}));
    // A retry waiting a second, as a throttled delivery leaves one.
    let event = crate::event::Event {
        id: "e2".into(),
        source: "x".into(),
        detail_type: "x".into(),
        account: super::ACCOUNT.into(),
        region: "us-east-1".into(),
        time_ms: START,
        resources: vec![],
        detail: json!({"n": 2}),
        replay_name: None,
    };
    let d = w
        .core
        .deliveries_for("000000000000/shop/r", &event, &event.doc(), &event.text());
    let throttled = crate::deliver::Failure {
        code: "ThrottlingException".into(),
        message: "slow down".into(),
        retryable: true,
    };
    for one in d {
        w.rt.block_on(w.core.failed(one, throttled.clone(), 1, &super::caller()));
    }
    w.core.flush().unwrap().unwrap().wait().unwrap();
    assert!(
        w.drain("q").is_empty(),
        "nothing is delivered before the timer runs"
    );
    w.core.db.take().unwrap().close();

    w.core = open(&dir, &w);
    assert_eq!((w.core.pending.len(), w.core.retries.len()), (1, 1));
    // The wake-up a start makes runs the accepted event at once and the retry when it is due.
    let next = w.tick(i64::MAX);
    assert_eq!(w.bodies("q")[0]["detail"], json!({"n": 1}));
    assert_eq!(next, Some(START + 1000));
    assert_eq!(w.wake_at(START + 1000), None);
    assert_eq!(w.drain("q").len(), 1, "the retry is delivered");
}
