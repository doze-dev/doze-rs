//! How a detail page speaks of its resource: the sentence built from its real fields, badges, verbs, what it is wired to; and
//! the sentence of a list. Queues and topics, against real services on a real bus.
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
use support::{keys, ts_keys};
use world::world;

const BASE: &str = "/api/v1/resources";

fn badges(d: &Value) -> Vec<String> {
    d["badges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["text"].as_str().unwrap().to_owned())
        .collect()
}

fn verbs(d: &Value) -> Vec<String> {
    d["verbs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap().to_owned())
        .collect()
}

fn wired(d: &Value) -> Vec<(String, String)> {
    d["wired"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| {
            (
                w["label"].as_str().unwrap().to_owned(),
                w["to"]["name"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[tokio::test]
async fn a_queue_says_what_it_holds_where_it_sends_what_fails_and_what_can_be_done() {
    let w = world("page-sqs", Mode::Soft, &[]);
    w.app(
        "sqs",
        "CreateQueue",
        &json!({"QueueName": "pick-lists-dlq"}),
    )
    .await;
    w.app(
        "sqs",
        "CreateQueue",
        &json!({"QueueName": "pick-lists", "Attributes": {"RedrivePolicy":
            "{\"deadLetterTargetArn\":\"arn:aws:sqs:us-east-1:000000000000:pick-lists-dlq\",\"maxReceiveCount\":5}"}}),
    )
    .await;
    for n in 0..2 {
        w.app(
            "sqs",
            "SendMessage",
            &json!({"QueueUrl": "http://127.0.0.1:4566/000000000000/pick-lists", "MessageBody": format!("m{n}")}),
        )
        .await;
    }
    w.app(
        "sqs",
        "SendMessage",
        &json!({"QueueUrl": "http://127.0.0.1:4566/000000000000/pick-lists-dlq", "MessageBody": "dead"}),
    )
    .await;
    let (status, d) = w.get(&format!("{BASE}/sqs/queues/pick-lists")).await;
    assert_eq!(status, 200, "{d}");
    assert_eq!(keys(&d), ts_keys("ResourceDetail"));
    assert_eq!(
        d["sentence"],
        "pick-lists has 2 messages waiting and 0 in flight. A message received more than 5 times moves to pick-lists-dlq."
    );
    assert_eq!(badges(&d), ["Standard"]);
    assert_eq!(keys(&d["badges"][0]), ts_keys("Badge"));
    assert_eq!(d["badges"][0]["tone"], "neutral");
    // Start DLQ redrive is always listed; the page turns it off for a queue that is no one's dead-letter queue.
    assert_eq!(
        verbs(&d),
        ["edit", "delete", "purge", "messages", "redrive"]
    );
    assert_eq!(keys(&d["verbs"][0]), ts_keys("VerbSpec"));
    let purge = &d["verbs"][2];
    assert_eq!(
        (
            purge["op"].clone(),
            purge["needs"].clone(),
            purge["danger"].clone()
        ),
        (json!("PurgeQueue"), json!("confirm"), json!(true))
    );
    assert_eq!(d["verbs"][1]["op"], Value::Null);
    assert_eq!(d["verbs"][1]["danger"], true);
    assert_eq!(
        wired(&d),
        [("Dead-letter queue".to_owned(), "pick-lists-dlq".to_owned())]
    );
    assert_eq!(keys(&d["wired"][0]), ts_keys("Wired"));
    assert_eq!(d["tags"], true);

    // The queue that others send their failures to says so, and has a redrive to start.
    let (_, dlq) = w.get(&format!("{BASE}/sqs/queues/pick-lists-dlq")).await;
    assert_eq!(
        dlq["sentence"],
        "pick-lists-dlq holds 1 message. It is the dead-letter queue of pick-lists."
    );
    assert_eq!(badges(&dlq), ["Standard", "Dead-letter queue"]);
    assert_eq!(dlq["badges"][1]["tone"], "info");
    assert_eq!(
        verbs(&dlq),
        ["edit", "delete", "purge", "messages", "redrive"]
    );

    // The list says what its page holds.
    let (_, list) = w.get(&format!("{BASE}/sqs/queues")).await;
    assert_eq!(
        list["sentence"],
        "2 queues hold 3 messages waiting. pick-lists-dlq is the only dead-letter queue with a message in it."
    );
    w.app(
        "sqs",
        "CreateQueue",
        &json!({"QueueName": "orders.fifo", "Attributes": {"FifoQueue": "true"}}),
    )
    .await;
    let (_, fifo) = w.get(&format!("{BASE}/sqs/queues/orders.fifo")).await;
    assert_eq!(badges(&fifo), ["FIFO"]);
    assert_eq!(
        fifo["sentence"],
        "orders.fifo has 0 messages waiting and 0 in flight."
    );
}

#[tokio::test]
async fn a_topic_says_where_it_delivers_and_a_subscription_how() {
    let w = world("page-sns", Mode::Soft, &[]);
    w.app("sns", "CreateTopic", &json!({"Name": "orders-created"}))
        .await;
    w.app("sns", "CreateTopic", &json!({"Name": "orders-archive"}))
        .await;
    let arn = "arn:aws:sns:us-east-1:000000000000:orders-created";
    // No subscription yet: nothing goes anywhere.
    let (_, d) = w.get(&format!("{BASE}/sns/topics/orders-archive")).await;
    assert_eq!(
        d["sentence"],
        "orders-archive has no subscriptions, so anything published to it goes nowhere."
    );
    for q in ["invoice-queue", "inventory-queue"] {
        w.app("sqs", "CreateQueue", &json!({"QueueName": q})).await;
        w.app(
            "sns",
            "Subscribe",
            &json!({"TopicArn": arn, "Protocol": "sqs",
                "Endpoint": format!("arn:aws:sqs:us-east-1:000000000000:{q}")}),
        )
        .await;
    }
    let (_, d) = w.get(&format!("{BASE}/sns/topics/orders-created")).await;
    assert_eq!(
        d["sentence"],
        "orders-created sends every publish to its 2 confirmed subscriptions: inventory-queue and invoice-queue."
    );
    assert_eq!(badges(&d), ["Standard"]);
    assert_eq!(verbs(&d), ["edit", "publish", "delete"]);
    let (_, list) = w.get(&format!("{BASE}/sns/topics")).await;
    assert_eq!(list["sentence"], "2 topics have 2 confirmed subscriptions.");
    // A confirmed subscription says where it delivers, and in what shape.
    let (_, subs) = w.get(&format!("{BASE}/sns/subscriptions")).await;
    let sub = subs["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["cells"][2].as_str().unwrap().ends_with(":invoice-queue"))
        .unwrap()["name"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, s) = w
        .get(&format!("{BASE}/sns/subscriptions/{}", world::encode(&sub)))
        .await;
    assert_eq!(
        s["sentence"],
        "Messages published to orders-created are delivered to invoice-queue (sqs), in SNS's JSON envelope."
    );
    assert_eq!(badges(&s), ["Confirmed", "sqs"]);
    assert_eq!(s["badges"][0]["tone"], "good");
    assert_eq!(verbs(&s), ["edit", "delete"]);
    assert_eq!(s["verbs"][1]["label"], "Unsubscribe");
    // An HTTP endpoint waits for its own confirmation.
    w.app(
        "sns",
        "Subscribe",
        &json!({"TopicArn": arn, "Protocol": "https", "Endpoint": "https://example.com/hook"}),
    )
    .await;
    let (_, d) = w.get(&format!("{BASE}/sns/topics/orders-created")).await;
    assert!(
        d["sentence"]
            .as_str()
            .unwrap()
            .ends_with("1 more waiting to be confirmed."),
        "{d}"
    );
}

#[tokio::test]
async fn a_kind_with_nothing_to_say_says_nothing_and_a_status_is_a_badge() {
    let w = world("page-none", Mode::Soft, &[]);
    w.app(
        "kinesis",
        "CreateStream",
        &json!({"StreamName": "clicks", "ShardCount": 1}),
    )
    .await;
    let (_, d) = w.get(&format!("{BASE}/kinesis/streams/clicks")).await;
    assert_eq!(d["sentence"], Value::Null);
    assert_eq!(verbs(&d), Vec::<String>::new());
    w.app(
        "dynamodb",
        "CreateTable",
        &json!({"TableName": "orders", "BillingMode": "PAY_PER_REQUEST",
            "AttributeDefinitions": [{"AttributeName": "id", "AttributeType": "S"}],
            "KeySchema": [{"AttributeName": "id", "KeyType": "HASH"}]}),
    )
    .await;
    let (_, d) = w.get(&format!("{BASE}/dynamodb/tables/orders")).await;
    assert_eq!(badges(&d), ["ACTIVE"]);
    assert_eq!(d["badges"][0]["tone"], "good");
    let (_, list) = w.get(&format!("{BASE}/dynamodb/tables")).await;
    assert_eq!(list["columns"][0]["badge"], "state");
    assert_eq!(list["sentence"], Value::Null);
}
