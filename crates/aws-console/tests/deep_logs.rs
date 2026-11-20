//! The deep pages' API, part two, against real services on a real bus as the caller `console`: a log stream read both ways, a
//! group searched, a Live Tail carried over server-sent events; a stack's events, template and change sets.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use std::time::Duration;

use aws_peers::Mode;
use axum::body::Body;
use axum::http::{Request, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use support::HOST;
use tower::ServiceExt;
use world::{World, field, world};

async fn deep(w: &World, path: &str, body: &Value) -> (u16, Value) {
    w.send("POST", &format!("/api/v1/deep/{path}"), body).await
}

/// The events of a server-sent stream, read until `enough` holds of them or `ms` pass.
async fn sse(w: &World, path: &str, enough: impl Fn(&str) -> bool, ms: u64) -> (u16, String) {
    let req = Request::builder()
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .body(Body::empty())
        .unwrap();
    let res = w.app.clone().oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    let mut body = res.into_body();
    let mut text = String::new();
    let _ = tokio::time::timeout(Duration::from_millis(ms), async {
        while let Some(Ok(frame)) = body.frame().await {
            if let Some(data) = frame.data_ref() {
                text.push_str(&String::from_utf8_lossy(data));
                if enough(&text) {
                    break;
                }
            }
        }
    })
    .await;
    (status, text)
}

#[tokio::test]
async fn a_stream_is_read_both_ways_a_group_searched_and_tailed() {
    let w = world("deep-logs", Mode::Soft, &[]);
    w.app(
        "logs",
        "CreateLogGroup",
        &json!({"logGroupName": "/app/api"}),
    )
    .await;
    for s in ["web-1", "web-2"] {
        w.app(
            "logs",
            "CreateLogStream",
            &json!({"logGroupName": "/app/api", "logStreamName": s}),
        )
        .await;
    }
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
        - 3_600_000;
    let events: Vec<Value> = (0..5)
        .map(|i| json!({"timestamp": now + i, "message": if i == 3 { "ERROR boom".to_owned() } else { format!("ok {i}") }}))
        .collect();
    w.app(
        "logs",
        "PutLogEvents",
        &json!({"logGroupName": "/app/api", "logStreamName": "web-1", "logEvents": events}),
    )
    .await;
    let (status, first) = deep(
        &w,
        "logs/GetLogEvents",
        &json!({"logGroupName": "/app/api", "logStreamName": "web-1", "startFromHead": true, "limit": 2}),
    )
    .await;
    assert_eq!(status, 200, "{first}");
    assert_eq!(first["events"][0]["message"], "ok 0");
    let (_, next) = deep(
        &w,
        "logs/GetLogEvents",
        &json!({"logGroupName": "/app/api", "logStreamName": "web-1", "startFromHead": true, "limit": 2, "nextToken": first["nextForwardToken"]}),
    )
    .await;
    assert_eq!(next["events"][0]["message"], "ok 2");
    let (_, back) = deep(
        &w,
        "logs/GetLogEvents",
        &json!({"logGroupName": "/app/api", "logStreamName": "web-1", "limit": 2, "nextToken": next["nextBackwardToken"]}),
    )
    .await;
    assert_eq!(back["events"][0]["message"], "ok 0");
    let (_, found) = deep(
        &w,
        "logs/FilterLogEvents",
        &json!({"logGroupName": "/app/api", "filterPattern": "ERROR", "startTime": now, "endTime": now + 10}),
    )
    .await;
    assert_eq!(found["events"].as_array().unwrap().len(), 1, "{found}");
    assert_eq!(found["events"][0]["logStreamName"], "web-1");

    // Live Tail: the session starts, and an event put while it is open arrives in an update.
    let w = std::sync::Arc::new(w);
    let writer = {
        let w = std::sync::Arc::clone(&w);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let at = i64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
            )
            .unwrap();
            w.app(
                "logs",
                "PutLogEvents",
                &json!({"logGroupName": "/app/api", "logStreamName": "web-2", "logEvents": [{"timestamp": at, "message": "ERROR live one"}]}),
            )
            .await;
        })
    };
    let (status, text) = sse(
        &w,
        "/api/v1/deep/logs/tail?group=%2Fapp%2Fapi&pattern=ERROR",
        |t| t.contains("live one"),
        5_000,
    )
    .await;
    writer.await.unwrap();
    assert_eq!(status, 200);
    assert!(text.contains("event: start"), "{text}");
    assert!(
        text.contains("event: update") && text.contains("ERROR live one"),
        "{text}"
    );
    assert!(w.live().contains(&"console logs StartLiveTail".to_owned()));
    let (status, _) = sse(&w, "/api/v1/deep/logs/tail", |_| true, 1_000).await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn a_stack_shows_its_events_template_and_change_sets() {
    let w = world("deep-cfn", Mode::Soft, &[]);
    let template = json!({"Resources": {"Queue": {"Type": "AWS::SQS::Queue", "Properties": {"QueueName": "deep-orders"}}}});
    w.app(
        "cloudformation",
        "CreateStack",
        &json!({"StackName": "larder", "TemplateBody": template.to_string()}),
    )
    .await;
    let mut events = Value::Null;
    for _ in 0..50 {
        events = deep(
            &w,
            "cloudformation/DescribeStackEvents",
            &json!({"StackName": "larder"}),
        )
        .await
        .1;
        if events.to_string().contains("CREATE_COMPLETE") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let list = events["StackEvents"].as_array().unwrap();
    assert!(list.len() >= 2, "{events}");
    assert_eq!(
        list[0]["LogicalResourceId"], "larder",
        "newest first: {events}"
    );
    let (status, got) = deep(
        &w,
        "cloudformation/GetTemplate",
        &json!({"StackName": "larder"}),
    )
    .await;
    assert_eq!(status, 200, "{got}");
    assert!(
        got["TemplateBody"]
            .as_str()
            .unwrap()
            .contains("deep-orders")
    );
    let (status, sets) = deep(
        &w,
        "cloudformation/ListChangeSets",
        &json!({"StackName": "larder"}),
    )
    .await;
    assert_eq!(status, 200, "{sets}");
    assert!(
        sets["Summaries"].as_array().is_none_or(Vec::is_empty),
        "{sets}"
    );
    let (_, detail) = w
        .get("/api/v1/resources/cloudformation/stacks/larder")
        .await;
    assert_eq!(detail["page"], "stack");
    assert_eq!(field(&detail, "Status")["value"], "CREATE_COMPLETE");
    // Not a JSON object: refused before any call.
    let (status, _) = deep(&w, "cloudformation/GetTemplate", &json!([1])).await;
    assert_eq!(status, 400);
}
