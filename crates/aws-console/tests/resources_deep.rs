//! A queue's own pages against real services on a real bus: its messages (send, peek, delete, purge), its settings and its
//! dead-letter redrive. Every call is the console's, on the queue the address names and no other.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "resources/deep.rs"]
mod deep;
mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use deep::{none, ok, op};
use serde_json::{Value, json};
use world::{World, world};

const BASE: &str = "/api/v1/resources/sqs/queues/orders";

/// A peek as the page asks for one: as long as it may, to hide what it reads for ten minutes (which the console overrules).
fn peek() -> Value {
    json!({"MaxNumberOfMessages": 10, "MessageSystemAttributeNames": ["All"],
        "MessageAttributeNames": ["All"], "VisibilityTimeout": 600, "WaitTimeSeconds": 20})
}

async fn queues(name: &str) -> World {
    let w = world(name, Mode::Soft, &[]);
    for q in ["orders", "other"] {
        w.app("sqs", "CreateQueue", &json!({"QueueName": q})).await;
    }
    w
}

#[tokio::test]
async fn a_queue_is_sent_to_and_peeked_without_hiding_its_messages() {
    let w = queues("deep-sqs-peek").await;
    let (_, detail) = w.get(BASE).await;
    let tabs: Vec<&str> = detail["deep"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert_eq!(tabs, ["messages", "redrive", "settings"]);
    // The page cannot send to another queue, whatever it names.
    let attrs = json!({"kind": {"DataType": "String", "StringValue": "new"}});
    let sent = ok(
        &w,
        BASE,
        "SendMessage",
        &json!({"MessageBody": "{\"id\":1}",
        "QueueUrl": "http://127.0.0.1:4566/000000000000/other", "MessageAttributes": attrs}),
    )
    .await;
    assert!(sent["MessageId"].is_string(), "{sent}");
    let first = ok(&w, BASE, "ReceiveMessage", &peek()).await;
    assert_eq!(first["Messages"].as_array().unwrap().len(), 1);
    let second = ok(&w, BASE, "ReceiveMessage", &peek()).await;
    let m = &second["Messages"][0];
    assert_eq!(m["Body"], "{\"id\":1}");
    assert_eq!(
        m["Attributes"]["ApproximateReceiveCount"], "2",
        "a peek counts as a receive"
    );
    assert_eq!(m["MessageAttributes"]["kind"]["StringValue"], "new");
    let other = w
        .app(
            "sqs",
            "GetQueueAttributes",
            &json!({"QueueUrl": "http://127.0.0.1:4566/000000000000/other",
        "AttributeNames": ["ApproximateNumberOfMessages"]}),
        )
        .await;
    assert_eq!(
        other["Attributes"]["ApproximateNumberOfMessages"], "0",
        "the send reached another queue"
    );
    ok(
        &w,
        BASE,
        "DeleteMessage",
        &json!({"ReceiptHandle": m["ReceiptHandle"]}),
    )
    .await;
    assert!(none(
        &ok(&w, BASE, "ReceiveMessage", &peek()).await["Messages"]
    ));
    let live = w.live();
    assert!(
        live.contains(&"console sqs SendMessage".to_owned()),
        "{live:?}"
    );
}

#[tokio::test]
async fn a_queue_is_purged_and_its_settings_changed_and_nothing_else_is_made() {
    let w = queues("deep-sqs-purge").await;
    ok(&w, BASE, "SendMessage", &json!({"MessageBody": "a"})).await;
    ok(&w, BASE, "PurgeQueue", &json!({})).await;
    assert!(none(
        &ok(&w, BASE, "ReceiveMessage", &peek()).await["Messages"]
    ));
    let attrs = json!({"Attributes": {"VisibilityTimeout": "45", "DelaySeconds": "2"}});
    ok(&w, BASE, "SetQueueAttributes", &attrs).await;
    let (_, detail) = w.get(BASE).await;
    let field = |label: &str| world::field(&detail, label)["value"].clone();
    assert_eq!(
        (field("Visibility timeout"), field("Delivery delay")),
        (json!("45"), json!("2"))
    );
    let (status, refused) = op(&w, BASE, "DeleteQueue", &json!({})).await;
    assert_eq!((status, refused["code"].as_str()), (404, Some("NOT_FOUND")));
    let (status, _) = op(&w, BASE, "SendMessage", &json!(["not an object"])).await;
    assert_eq!(status, 400);
    assert!(w.live().contains(&"console sqs PurgeQueue".to_owned()));
}

#[tokio::test]
async fn a_dead_letter_queue_moves_its_messages_back_and_lists_the_task() {
    let w = world("deep-redrive", Mode::Soft, &[]);
    w.app("sqs", "CreateQueue", &json!({"QueueName": "orders-dlq"}))
        .await;
    let arn = |q: &str| format!("arn:aws:sqs:us-east-1:000000000000:{q}");
    let policy =
        json!({"deadLetterTargetArn": arn("orders-dlq"), "maxReceiveCount": 1}).to_string();
    let made = json!({"QueueName": "orders", "Attributes": {"RedrivePolicy": policy}});
    w.app("sqs", "CreateQueue", &made).await;
    let dlq = "/api/v1/resources/sqs/queues/orders-dlq";
    for body in ["a", "b"] {
        ok(&w, dlq, "SendMessage", &json!({"MessageBody": body})).await;
    }
    let started = ok(
        &w,
        dlq,
        "StartMessageMoveTask",
        &json!({"DestinationArn": arn("orders")}),
    )
    .await;
    assert!(started["TaskHandle"].is_string(), "{started}");
    let mut moved = Value::Null;
    for _ in 0..50 {
        let tasks = ok(&w, dlq, "ListMessageMoveTasks", &json!({"MaxResults": 10})).await;
        moved = tasks["Results"][0].clone();
        if moved["Status"] == "COMPLETED" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(moved["Status"], "COMPLETED", "{moved}");
    assert_eq!(moved["SourceArn"], arn("orders-dlq"));
    assert_eq!(moved["ApproximateNumberOfMessagesMoved"], 2);
}

#[tokio::test]
async fn a_move_task_is_cancelled_only_from_the_page_of_the_queue_it_moves_from() {
    let w = world("deep-redrive-cancel", Mode::Soft, &[]);
    let arn = |q: &str| format!("arn:aws:sqs:us-east-1:000000000000:{q}");
    for (q, dlq) in [("orders", "orders-dlq"), ("billing", "billing-dlq")] {
        w.app("sqs", "CreateQueue", &json!({"QueueName": dlq}))
            .await;
        let policy = json!({"deadLetterTargetArn": arn(dlq), "maxReceiveCount": 1}).to_string();
        let made = json!({"QueueName": q, "Attributes": {"RedrivePolicy": policy}});
        w.app("sqs", "CreateQueue", &made).await;
    }
    let orders = "/api/v1/resources/sqs/queues/orders-dlq";
    let billing = "/api/v1/resources/sqs/queues/billing-dlq";
    // Three at one a second, to a named queue (sent straight here, they have no source to go back to): the move is still
    // running when it is cancelled (only a running one lists its handle).
    for body in ["a", "b", "c"] {
        ok(&w, orders, "SendMessage", &json!({"MessageBody": body})).await;
    }
    let started = ok(
        &w,
        orders,
        "StartMessageMoveTask",
        &json!({"DestinationArn": arn("orders"), "MaxNumberOfMessagesPerSecond": 1}),
    )
    .await;
    let handle = started["TaskHandle"].as_str().unwrap().to_owned();
    // From another queue's page the handle is refused before SQS is asked to cancel anything.
    let (status, answer) = op(
        &w,
        billing,
        "CancelMessageMoveTask",
        &json!({"TaskHandle": handle}),
    )
    .await;
    assert_eq!(
        (status, answer["code"].as_str()),
        (400, Some("NOT_THIS_RESOURCE")),
        "{answer}"
    );
    let (status, answer) = op(&w, billing, "CancelMessageMoveTask", &json!({})).await;
    assert_eq!(answer["code"], "NOT_THIS_RESOURCE", "{status} {answer}");
    assert!(
        !w.live()
            .contains(&"console sqs CancelMessageMoveTask".to_owned())
    );
    // From its own page it is cancelled.
    let (status, answer) = op(
        &w,
        orders,
        "CancelMessageMoveTask",
        &json!({"TaskHandle": handle}),
    )
    .await;
    assert_eq!(status, 200, "{answer}");
    assert!(
        w.live()
            .contains(&"console sqs CancelMessageMoveTask".to_owned())
    );
}
