//! A subscription's page of its own: Subscribe with its attributes, its settings changed one attribute a call, and ConfirmSubscription by token,
//! each bound to the resource it acts on. Against real services on a real bus.
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

const BASE: &str = "/api/v1/resources";
const ACCOUNT: &str = "000000000000";

fn topic_arn(name: &str) -> String {
    format!("arn:aws:sns:us-east-1:{ACCOUNT}:{name}")
}

#[tokio::test]
async fn a_subscription_is_made_with_its_attributes_changed_one_at_a_time_and_confirmed_by_token() {
    let w = world("sns-subs", Mode::Enforce, &[]);
    w.app("sns", "CreateTopic", &json!({"Name": "orders-created"}))
        .await;
    w.app("sqs", "CreateQueue", &json!({"QueueName": "invoice-queue"}))
        .await;
    let arn = topic_arn("orders-created");
    let (status, made) = w
        .send(
            "POST",
            &format!("{BASE}/sns/subscriptions/op/Subscribe"),
            &json!({"TopicArn": arn, "Protocol": "sqs", "ReturnSubscriptionArn": true,
                "Endpoint": format!("arn:aws:sqs:us-east-1:{ACCOUNT}:invoice-queue"),
                "Attributes": {"RawMessageDelivery": "true", "FilterPolicy": "{\"event\":[\"order_placed\"]}"}}),
        )
        .await;
    assert_eq!(status, 200, "{made}");
    let sub_arn = made["SubscriptionArn"].as_str().unwrap().to_owned();
    let one = format!("{BASE}/sns/subscriptions/{}", encode(&sub_arn));
    let (_, d) = w.get(&one).await;
    assert_eq!(field(&d, "Raw message delivery")["value"], "Enabled");
    assert_eq!(field(&d, "Topic ARN")["value"], arn);
    let policy: Value =
        serde_json::from_str(field(&d, "Filter policy")["value"].as_str().unwrap()).unwrap();
    assert_eq!(policy, json!({"event": ["order_placed"]}));
    // SetSubscriptionAttributes acts on this subscription, one attribute a call.
    let (status, set) = w
        .send(
            "POST",
            &format!("{one}/op/SetSubscriptionAttributes"),
            &json!({"SubscriptionArn": "arn:aws:sns:us-east-1:000000000000:alerts:other", "AttributeName": "RawMessageDelivery", "AttributeValue": "false"}),
        )
        .await;
    assert_eq!(status, 200, "{set}");
    let (_, d) = w.get(&one).await;
    assert_eq!(field(&d, "Raw message delivery")["value"], "Disabled");
    let (status, bad) = w
        .send(
            "POST",
            &format!("{one}/op/SetSubscriptionAttributes"),
            &json!({"AttributeName": "FilterPolicyScope", "AttributeValue": "Body"}),
        )
        .await;
    assert_eq!(status, 400, "{bad}");
    // An email address waits for a confirmation nobody is sent: the pending row is listed with its topic, and a token that is
    // not its is the service's refusal.
    w.app(
        "sns",
        "Subscribe",
        &json!({"TopicArn": arn, "Protocol": "email", "Endpoint": "ops@larder.example"}),
    )
    .await;
    let (_, topic) = w.get(&format!("{BASE}/sns/topics/orders-created")).await;
    let rows = sub(&topic, "Subscriptions")["rows"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter().any(
            |r| r["cells"][0] == "PendingConfirmation" && r["cells"][2] == "ops@larder.example"
        ),
        "{topic}"
    );
    let (status, refused) = w
        .send(
            "POST",
            &format!("{BASE}/sns/subscriptions/op/ConfirmSubscription"),
            &json!({"TopicArn": arn, "Token": "not-the-token"}),
        )
        .await;
    assert_eq!(status, 400, "{refused}");
    assert_eq!(refused["code"], "InvalidParameter");
    // The subscription's other operations are not the page's to make.
    let (status, gone) = w
        .send(
            "POST",
            &format!("{one}/op/Unsubscribe"),
            &json!({"SubscriptionArn": sub_arn}),
        )
        .await;
    assert_eq!((status, gone["code"].as_str()), (404, Some("NOT_FOUND")));
}
