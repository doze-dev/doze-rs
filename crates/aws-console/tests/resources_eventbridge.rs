//! EventBridge's pages: the kinds the console added (replays, API destinations), the kind operations the pages make (a rule with its
//! targets, a bus with its tags, the events put on a bus), how a rule, a bus and an archive speak, and which calls a bus owns.
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
use world::{encode, field, sub, world};

const BASE: &str = "/api/v1/resources/eventbridge";
const SQS: &str = "arn:aws:sqs:us-east-1:000000000000:inbox";

fn op(kind: &str, operation: &str) -> String {
    format!("{BASE}/{kind}/op/{operation}")
}

fn texts(d: &Value, key: &str) -> Vec<String> {
    d[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().or(v["text"].as_str()).unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn a_rule_is_made_with_its_tags_and_targets_and_a_bus_with_its_settings_by_the_kinds_own_operations()
 {
    let w = world("eb-ops", Mode::Soft, &[]);
    let (status, bus) = w
        .send(
            "POST",
            &op("buses", "CreateEventBus"),
            &json!({"Name": "larder", "Description": "the stack", "KmsKeyIdentifier": "alias/app",
                "Tags": [{"Key": "Environment", "Value": "dev"}]}),
        )
        .await;
    assert_eq!(status, 200, "{bus}");
    let (_, detail) = w.get(&format!("{BASE}/buses/larder")).await;
    assert_eq!(field(&detail, "KMS key")["value"], "alias/app");
    assert_eq!(detail["tags"], true);
    let (_, tags) = w.get(&format!("{BASE}/buses/larder/tags")).await;
    assert_eq!(tags["tags"][0]["key"], "Environment");
    // The archive of a bus is its own call, after the bus exists.
    let (status, archive) = w
        .send(
            "POST",
            &op("buses", "CreateArchive"),
            &json!({"ArchiveName": "larder-all", "EventSourceArn": bus["EventBusArn"]}),
        )
        .await;
    assert_eq!(status, 200, "{archive}");
    // A rule: PutRule with its tags, then PutTargets with the retry policy; the rule's page reads both.
    let (status, made) = w
        .send(
            "POST",
            &op("rules", "PutRule"),
            &json!({"Name": "on-order", "EventBusName": "larder", "EventPattern": r#"{"source":["larder.orders"]}"#,
                "State": "ENABLED", "Tags": [{"Key": "Team", "Value": "checkout"}]}),
        )
        .await;
    assert_eq!(status, 200, "{made}");
    let (status, put) = w
        .send(
            "POST",
            &op("rules", "PutTargets"),
            &json!({"Rule": "on-order", "EventBusName": "larder", "Targets": [{"Id": "inbox", "Arn": SQS,
                "RetryPolicy": {"MaximumRetryAttempts": 3, "MaximumEventAgeInSeconds": 600}}]}),
        )
        .await;
    assert_eq!(
        (status, put["FailedEntryCount"].as_i64()),
        (200, Some(0)),
        "{put}"
    );
    let (_, listed) = w
        .send(
            "POST",
            &op("rules", "ListTargetsByRule"),
            &json!({"Rule": "on-order", "EventBusName": "larder"}),
        )
        .await;
    assert_eq!(
        listed["Targets"][0]["RetryPolicy"]["MaximumRetryAttempts"],
        3
    );
    // Disable and enable are the kind's.
    let one = format!("{BASE}/rules/{}", encode("larder/on-order"));
    let off = json!({"Name": "on-order", "EventBusName": "larder"});
    let (status, _) = w.send("POST", &op("rules", "DisableRule"), &off).await;
    assert_eq!(status, 200);
    let (_, rule) = w.get(&one).await;
    assert_eq!(texts(&rule, "badges"), ["Disabled"]);
    assert_eq!(texts(&rule, "verbs"), ["enable", "delete"]);
    // The pattern is tested against a sample event without a rule.
    let event = json!({"id": "1", "account": "000000000000", "source": "larder.orders", "time": "2026-11-08T08:13:41Z",
        "region": "us-east-1", "resources": [], "detail-type": "OrderPlaced", "detail": {}});
    let (status, tested) = w
        .send(
            "POST",
            &op("rules", "TestEventPattern"),
            &json!({"EventPattern": r#"{"source":["larder.orders"]}"#, "Event": event.to_string()}),
        )
        .await;
    assert_eq!((status, &tested["Result"]), (200, &json!(true)), "{tested}");
    // What a kind does not list is not made, here a bus's delete through a rule's address.
    let (status, refused) = w
        .send(
            "POST",
            &op("rules", "DeleteEventBus"),
            &json!({"Name": "larder"}),
        )
        .await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (404, Some("NOT_IN_PAGE"))
    );
}

#[tokio::test]
async fn a_rule_a_bus_and_an_archive_say_what_they_do() {
    let w = world("eb-says", Mode::Soft, &[]);
    w.app("events", "CreateEventBus", &json!({"Name": "larder"}))
        .await;
    w.app("events", "PutRule", &json!({"Name": "on-order", "EventBusName": "larder", "EventPattern": r#"{"source":["a"]}"#}))
        .await;
    w.app(
        "events",
        "PutRule",
        &json!({"Name": "nightly", "ScheduleExpression": "rate(1 day)"}),
    )
    .await;
    let (_, bare) = w
        .get(&format!("{BASE}/rules/{}", encode("larder/on-order")))
        .await;
    assert_eq!(
        bare["sentence"],
        "on-order is enabled on the larder bus and matches events by its pattern. It has no target yet, so it does nothing."
    );
    assert_eq!(texts(&bare, "verbs"), ["disable", "delete"]);
    w.app(
        "events",
        "PutTargets",
        &json!({"Rule": "on-order", "EventBusName": "larder", "Targets": [{"Id": "t", "Arn": SQS}]}),
    )
    .await;
    let (_, fed) = w
        .get(&format!("{BASE}/rules/{}", encode("larder/on-order")))
        .await;
    assert_eq!(
        fed["sentence"],
        "on-order is enabled on the larder bus and sends what matches its pattern to 1 target: inbox."
    );
    let (_, tick) = w.get(&format!("{BASE}/rules/nightly")).await;
    assert!(
        tick["sentence"]
            .as_str()
            .unwrap()
            .contains("on the default bus and runs on rate(1 day)."),
        "{tick}"
    );
    // A bus counts its rules and its archives, and a custom one says what names reach it.
    let arn = "arn:aws:events:us-east-1:000000000000:event-bus/larder";
    w.app(
        "events",
        "CreateArchive",
        &json!({"ArchiveName": "all", "EventSourceArn": arn, "RetentionDays": 30}),
    )
    .await;
    let (_, bus) = w.get(&format!("{BASE}/buses/larder")).await;
    assert_eq!(
        bus["sentence"],
        "larder has 1 rule and 1 archive. Events sent to this bus by name reach only its rules, never the ones on default."
    );
    assert_eq!(texts(&bus, "badges"), ["Custom event bus"]);
    assert_eq!(texts(&bus, "verbs"), ["send", "delete"]);
    assert_eq!(sub(&bus, "Archives")["rows"][0]["cells"][2], "30");
    let (_, default) = w.get(&format!("{BASE}/buses/default")).await;
    assert_eq!(texts(&default, "badges"), ["Default event bus"]);
    assert_eq!(texts(&default, "verbs"), ["send"]);
    let (_, archive) = w.get(&format!("{BASE}/archives/all")).await;
    assert_eq!(
        archive["sentence"],
        "all keeps every event from the larder bus for 30 days and holds 0 events."
    );
}

#[tokio::test]
async fn replays_and_api_destinations_are_kinds_with_pages_of_their_own() {
    let w = world("eb-kinds", Mode::Soft, &[]);
    // Nothing yet: an empty list, and a name that is not there is a refusal, not a rule's page.
    let (status, list) = w.get(&format!("{BASE}/replays")).await;
    assert_eq!(
        (status, list["rows"].as_array().unwrap().len()),
        (200, 0),
        "{list}"
    );
    let (status, _) = w.get(&format!("{BASE}/api-destinations/none")).await;
    assert_eq!(status, 400);
    let (status, conn) = w
        .send(
            "POST",
            &op("api-destinations", "CreateConnection"),
            &json!({"Name": "finance-api", "AuthorizationType": "API_KEY",
                "AuthParameters": {"ApiKeyAuthParameters": {"ApiKeyName": "X-Api-Key", "ApiKeyValue": "s3cret"}}}),
        )
        .await;
    assert_eq!(status, 200, "{conn}");
    let (status, made) = w
        .send(
            "POST",
            &op("api-destinations", "CreateApiDestination"),
            &json!({"Name": "finance-webhook", "ConnectionArn": conn["ConnectionArn"], "HttpMethod": "POST",
                "InvocationEndpoint": "http://127.0.0.1:9/refunds", "InvocationRateLimitPerSecond": 300}),
        )
        .await;
    assert_eq!(status, 200, "{made}");
    let (_, list) = w.get(&format!("{BASE}/api-destinations")).await;
    assert_eq!(
        list["rows"][0]["cells"],
        json!([
            "ACTIVE",
            "http://127.0.0.1:9/refunds",
            "POST",
            conn["ConnectionArn"],
            "300"
        ])
    );
    let (_, one) = w
        .get(&format!("{BASE}/api-destinations/finance-webhook"))
        .await;
    assert_eq!(field(&one, "Rate limit per second")["value"], "300");
    // The connection is read through the kind, and its secret is never in the answer.
    let (_, conns) = w
        .send(
            "POST",
            &op("api-destinations", "ListConnections"),
            &json!({}),
        )
        .await;
    assert_eq!(conns["Connections"][0]["ConnectionState"], "AUTHORIZED");
    assert!(!conns.to_string().contains("s3cret"));
}

#[tokio::test]
async fn events_put_on_a_bus_are_among_its_recent_calls() {
    let w = world("eb-calls", Mode::Soft, &[]);
    w.app("events", "CreateEventBus", &json!({"Name": "larder"}))
        .await;
    for bus in ["larder", "default"] {
        let (status, put) = w
            .send(
                "POST",
                &op("buses", "PutEvents"),
                &json!({"Entries": [{"EventBusName": bus, "Source": "larder.orders", "DetailType": "OrderPlaced", "Detail": "{}"}]}),
            )
            .await;
        assert_eq!(
            (status, put["FailedEntryCount"].as_i64()),
            (200, Some(0)),
            "{put}"
        );
    }
    // An app's call, not the console's: the one with no bus named is the default bus's.
    w.app(
        "events",
        "PutEvents",
        &json!({"Entries": [{"Source": "a", "DetailType": "b", "Detail": "{}"}]}),
    )
    .await;
    let (_, larder) = w.get(&format!("{BASE}/buses/larder")).await;
    let calls: Vec<String> = larder["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["action"].as_str().unwrap().to_owned())
        .collect();
    assert!(calls.contains(&"PutEvents".to_owned()), "{calls:?}");
    let (_, default) = w.get(&format!("{BASE}/buses/default")).await;
    let puts = default["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["action"] == "PutEvents")
        .count();
    assert_eq!(puts, 2, "{default}");
}
