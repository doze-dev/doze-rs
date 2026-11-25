//! What Live's map is drawn from: the resources of the services apps called, and how they are wired: a dead-letter queue, a topic's
//! subscription, a rule's target, a bucket's notification. A service no app called is not looked in.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use std::time::Duration;

use aws_gateway::{Did, Finished};
use aws_peers::Mode;
use serde_json::{Value, json};
use support::{keys, ts_keys};
use world::{World, world};

fn wire(signing: &'static str) -> Finished<'static> {
    Finished {
        request_id: "req",
        signing_name: signing,
        sdk_id: "SQS",
        action: "SendMessage",
        member: "",
        resource: "",
        access_key_id: "AKIAAPP",
        region: "us-east-1",
        status: 200,
        code: "",
        took: Duration::from_micros(900),
        would_be_denied: false,
        user_agent: "",
        message: "",
        function_error: "",
        answer_bytes: None,
        did: Did::NONE,
        lab: None,
    }
}

fn arn(service: &str, name: &str) -> String {
    format!("arn:aws:{service}:us-east-1:000000000000:{name}")
}

async fn larder(w: &World) {
    w.app("sqs", "CreateQueue", &json!({"QueueName": "jobs-dlq"}))
        .await;
    let policy =
        json!({"deadLetterTargetArn": arn("sqs", "jobs-dlq"), "maxReceiveCount": "5"}).to_string();
    let jobs = w
        .app(
            "sqs",
            "CreateQueue",
            &json!({"QueueName": "jobs", "Attributes": {"RedrivePolicy": policy}}),
        )
        .await["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    w.app(
        "sqs",
        "SendMessage",
        &json!({"QueueUrl": jobs, "MessageBody": "a"}),
    )
    .await;
    w.app("sqs", "CreateQueue", &json!({"QueueName": "idle"}))
        .await;
    let topic = w
        .app("sns", "CreateTopic", &json!({"Name": "orders"}))
        .await["TopicArn"]
        .as_str()
        .unwrap()
        .to_owned();
    w.app(
        "sns",
        "Subscribe",
        &json!({"TopicArn": topic, "Protocol": "sqs", "Endpoint": arn("sqs", "jobs")}),
    )
    .await;
}

fn edge<'a>(v: &'a Value, from: &str, to: &str) -> Option<&'a Value> {
    v["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["from"] == from && e["to"] == to)
}

#[tokio::test]
async fn the_resources_apps_called_and_how_they_are_wired() {
    let w = world("wiring", Mode::Soft, &[]);
    w.state.recorder.record(&wire("sqs"), 1);
    w.state.recorder.record(&wire("sns"), 2);
    larder(&w).await;
    let (status, g) = w.get("/api/v1/live/wiring").await;
    assert_eq!(status, 200, "{g}");
    assert_eq!(keys(&g), ts_keys("Wiring"));
    assert_eq!(g["looked"], json!(["sqs", "sns"]));
    let jobs = g["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "sqs/queues/jobs")
        .unwrap();
    assert_eq!(keys(jobs), ts_keys("WireNode"));
    assert_eq!(jobs["sentence"], "1 waiting · 0 in flight");
    // The queue its redrive policy names, with the limit.
    let dlq = edge(&g, "sqs/queues/jobs", "sqs/queues/jobs-dlq").unwrap();
    assert_eq!(keys(dlq), ts_keys("WireEdge"));
    assert_eq!(
        (dlq["label"].as_str(), dlq["note"].as_str()),
        (Some("redrive"), Some("after 5 receives"))
    );
    // A topic delivers to the queue it has a subscription for.
    assert_eq!(
        edge(&g, "sns/topics/orders", "sqs/queues/jobs").unwrap()["label"],
        "subscription"
    );
    assert_eq!(
        g["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == "sns/topics/orders")
            .unwrap()["sentence"],
        "1 subscription"
    );
    // A queue nothing is wired to is a node with no line.
    assert!(
        g["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["id"] == "sqs/queues/idle")
    );
    assert!(
        g["edges"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["from"] != "sqs/queues/idle" && e["to"] != "sqs/queues/idle")
    );
}

#[tokio::test]
async fn a_service_no_app_called_is_not_looked_in() {
    let w = world("wiring-asleep", Mode::Soft, &[]);
    w.state.recorder.record(&wire("sqs"), 1);
    larder(&w).await;
    let (_, g) = w.get("/api/v1/live/wiring").await;
    assert_eq!(g["looked"], json!(["sqs"]));
    assert!(
        g["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n["service"] == "sqs"),
        "{g}"
    );
}
