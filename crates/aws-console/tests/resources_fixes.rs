//! Resources pages that once read a service's answer wrongly, each against real services on a real bus.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use serde_json::json;
use world::{encode, field, world};

#[tokio::test]
async fn two_subscriptions_not_yet_confirmed_are_two_rows_with_their_own_pages() {
    let w = world("sns", Mode::Soft, &[]);
    let topic = "arn:aws:sns:us-east-1:000000000000:orders";
    w.round_trip(
        "/api/v1/resources/sns/topics",
        &json!({"Name": "orders"}),
        "orders",
    )
    .await;
    for who in ["ann@example.com", "bob@example.com"] {
        let (status, _) = w
            .send(
                "POST",
                "/api/v1/resources/sns/subscriptions",
                &json!({"TopicArn": topic, "Protocol": "email", "Endpoint": who}),
            )
            .await;
        assert_eq!(status, 201);
    }
    // The list: each is its own row, named by what tells it apart, not by the ARN SNS lists for both.
    let (_, list) = w.get("/api/v1/resources/sns/subscriptions").await;
    let rows = list["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let names: Vec<&str> = rows.iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_ne!(names[0], names[1]);
    assert!(names.iter().all(|n| !n.contains("PendingConfirmation:")));
    assert!(
        rows.iter()
            .any(|r| r["label"] == "ann@example.com (pending)")
    );
    // Each opens, and says what it is.
    for who in ["ann@example.com", "bob@example.com"] {
        let name = format!("pending|{topic}|email|{who}");
        let (status, detail) = w
            .get(&format!(
                "/api/v1/resources/sns/subscriptions/{}",
                encode(&name)
            ))
            .await;
        assert_eq!(status, 200, "{detail}");
        assert_eq!(field(&detail, "Endpoint")["value"], who);
        assert_eq!(field(&detail, "Protocol")["value"], "email");
        assert_eq!(field(&detail, "Pending confirmation")["value"], "Yes");
        assert_eq!(
            field(&detail, "Topic")["link"],
            json!({"service": "sns", "kind": "topics", "name": "orders"})
        );
    }
    // The topic's own list links each row to its page.
    let (_, topic_page) = w.get("/api/v1/resources/sns/topics/orders").await;
    let links: Vec<&str> = topic_page["subs"][0]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["link"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(links.len(), 2);
    assert!(links.iter().all(|l| l.starts_with("pending|")));
    assert_ne!(links[0], links[1]);
    // One that is gone says so.
    let (status, gone) = w
        .get(&format!(
            "/api/v1/resources/sns/subscriptions/{}",
            encode(&format!("pending|{topic}|email|eve@example.com"))
        ))
        .await;
    assert_eq!(status, 404, "{gone}");
}
