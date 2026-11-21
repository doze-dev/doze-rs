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

/// Model `DeleteSecret`: a secret marked for deletion cannot be read ("you cannot retrieve the secret value") until
/// `RestoreSecret` removes the stamp; `DescribeSecret.DeletedDate` is the stamp, so the page says "marked", not "scheduled for".
#[tokio::test]
async fn a_secret_marked_for_deletion_offers_neither_reveal_nor_delete_but_a_way_back() {
    let w = world("secrets", Mode::Soft, &[]);
    let base = "/api/v1/resources/secretsmanager/secrets";
    let name = w
        .round_trip(
            base,
            &json!({"Name": "larder/stripe", "SecretString": "sk_live_1"}),
            "larder/stripe",
        )
        .await;
    let one = format!("{base}/{}", encode(&name));
    let (_, before) = w.get(&one).await;
    assert_eq!(before["reveal"], "Reveal value");
    assert_eq!(before["delete"]["verb"], "Schedule deletion");
    assert_eq!(field(&before, "Value")["hidden"], true);
    let (status, _) = w.send("DELETE", &one, &json!({})).await;
    assert_eq!(status, 200);
    // Still listed (it is there), with the day it was marked.
    let (_, list) = w.get(base).await;
    let row = &list["rows"][0];
    assert_eq!(row["name"], "larder/stripe");
    assert!(row["cells"][4].is_string(), "{row}");
    let (status, marked) = w.get(&one).await;
    assert_eq!(status, 200, "{marked}");
    assert!(field(&marked, "Marked for deletion")["value"].is_string());
    assert!(marked["reveal"].is_null() && marked["delete"].is_null());
    assert_eq!(field(&marked, "Value")["hidden"], false);
    assert!(
        field(&marked, "Value")["value"]
            .as_str()
            .unwrap()
            .contains("Not readable")
    );
    assert_eq!(marked["deep"][0]["operations"], json!(["RestoreSecret"]));
    // Cancel the deletion.
    let (status, _) = w
        .send("POST", &format!("{one}/op/RestoreSecret"), &json!({}))
        .await;
    assert_eq!(status, 200);
    let (_, back) = w.get(&one).await;
    assert_eq!(back["reveal"], "Reveal value");
    assert!(field(&back, "Marked for deletion")["value"].is_null());
}
