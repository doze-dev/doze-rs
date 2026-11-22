//! A timestamp written as text (ISO 8601) in a preview is one word on the command line and a string in boto3, as a person types it.
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
use serde_json::json;
use world::world;

#[tokio::test]
async fn a_timestamp_written_as_text_is_one_word_on_the_command_line() {
    let w = world("preview-timestamp", Mode::Soft, &[]);
    let input = json!({
        "ReplayName": "redo",
        "EventSourceArn": "arn:aws:events:us-east-1:000000000000:archive/a",
        "EventStartTime": "2026-11-07T00:00:00Z",
        "EventEndTime": "2026-11-08T00:00:00Z",
        "Destination": {"Arn": "arn:aws:events:us-east-1:000000000000:event-bus/larder"}
    });
    let (status, p) = w
        .send(
            "POST",
            "/api/v1/preview",
            &json!({"service": "events", "operation": "StartReplay", "input": input}),
        )
        .await;
    assert_eq!(status, 200, "{p}");
    assert_eq!(p["json"]["EventStartTime"], "2026-11-07T00:00:00Z");
    let cli = p["cli"].as_str().unwrap();
    assert!(
        cli.contains(" --event-start-time 2026-11-07T00:00:00Z"),
        "{cli}"
    );
    assert!(!cli.contains("'\"2026"), "{cli}");
    let boto = p["boto3"].as_str().unwrap();
    assert!(
        boto.contains(r#"EventStartTime="2026-11-07T00:00:00Z""#),
        "{boto}"
    );
}
