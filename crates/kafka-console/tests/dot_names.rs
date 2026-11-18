//! A group (or a topic) named `.` or `..` over a real broker: its path segment addresses it and never another route. The
//! 2026-11-13 security audit deleted the offsets of a group named `offsets` by asking for those of a group named `.`, because
//! the page encoded the name with encodeURIComponent and the browser collapsed `/groups/./offsets/delete`. The page now writes
//! such a name `!.` (`web/src/api/path.ts`), the server turns it into `%2E` before routing (`app::undot`), and `%2E` decodes to
//! the name.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_write;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use kafka_wire::StrBytes;
use kafka_wire::api::Request as KRequest;
use kafka_wire::messages::offset_commit_request::{
    OffsetCommitRequestPartition, OffsetCommitRequestTopic,
};
use kafka_wire::messages::{GroupId, OffsetCommitRequest, TopicName};
use serde_json::Value;
use support::{HOST, send};

async fn call(app: &Router, method: &str, path: &str, body: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .header("sec-fetch-site", "same-origin")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap();
    let (res, text) = send(app, req).await;
    let value = serde_json::from_str(&text).unwrap_or_else(|_| panic!("{path}: {text}"));
    (res.status(), value)
}

fn commit(group: &str, topic: &str) -> KRequest {
    let mut p = OffsetCommitRequestPartition::default();
    p.partition_index = 0;
    p.committed_offset = 1;
    p.committed_leader_epoch = -1;
    let mut t = OffsetCommitRequestTopic::default();
    t.name = TopicName(StrBytes::from_string(topic.to_owned()));
    t.partitions = vec![p];
    let mut c = OffsetCommitRequest::default();
    c.group_id = GroupId(StrBytes::from_string(group.to_owned()));
    c.generation_id_or_member_epoch = -1;
    c.topics = vec![t];
    KRequest::OffsetCommit(c)
}

/// The topics a group has committed offsets for, as the Lag tab shows them.
async fn committed(app: &Router, segment: &str) -> Vec<String> {
    let (s, v) = call(app, "GET", &format!("/api/v1/groups/{segment}"), "").await;
    assert_eq!(s, StatusCode::OK, "{segment}: {v}");
    v["topics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            t["name"]
                .as_str()
                .or(t["topic"].as_str())
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

#[tokio::test]
async fn a_group_named_dot_or_dot_dot_is_read_and_written_by_its_own_segment_and_never_another_route()
 {
    let w = support_write::world("dot-names", |_, _| {}).await;
    w.create_topic("orders", 1, &[]).await;
    w.create_topic("payments", 1, &[]).await;
    for group in [".", "..", "offsets"] {
        let _ = w.ask(8, commit(group, "orders")).await;
        let _ = w.ask(8, commit(group, "payments")).await;
    }
    let app = w.router();
    // Read: `%2E` (as curl sends it) and `!.` (as the page sends it) are the group `.`; `!..` and `%2E%2E` are `..`.
    for (segment, id) in [
        ("%2E", "."),
        ("!.", "."),
        ("%2E%2E", ".."),
        ("!..", ".."),
        ("offsets", "offsets"),
    ] {
        let (s, v) = call(&app, "GET", &format!("/api/v1/groups/{segment}"), "").await;
        assert_eq!(s, StatusCode::OK, "{segment}: {v}");
        assert_eq!(v["head"]["id"], id, "{segment}: {v}");
        let (s, v) = call(&app, "GET", &format!("/api/v1/groups/{segment}/story"), "").await;
        assert_eq!(s, StatusCode::OK, "{segment}: {v}");
    }
    assert_eq!(committed(&app, "offsets").await.len(), 2);
    // Write: the audit's request. The offsets of `.` go; those of the group named `offsets` stay.
    let (s, form) = call(&app, "GET", "/api/v1/groups/!./offsets/delete", "").await;
    assert_eq!(s, StatusCode::OK, "{form}");
    let (s, done) = call(
        &app,
        "POST",
        "/api/v1/groups/!./offsets/delete",
        r#"{"topics":["orders"]}"#,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{done}");
    assert_eq!(done["outcome"], "done", "{done}");
    assert_eq!(committed(&app, "%2E").await.len(), 1, "only orders went");
    assert_eq!(
        committed(&app, "offsets").await.len(),
        2,
        "the group named offsets was not touched"
    );
    // And `..` is deleted by its own segment, as curl writes it; `offsets` is still there.
    let (s, done) = call(&app, "POST", "/api/v1/groups/%2E%2E/delete", "{}").await;
    assert_eq!(s, StatusCode::OK, "{done}");
    assert_eq!(done["outcome"], "done", "{done}");
    let (_, list) = call(&app, "GET", "/api/v1/groups", "").await;
    let ids: Vec<&str> = list["view"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [".", "offsets"], "{list}");
    w.stop().await;
}

#[tokio::test]
async fn a_topic_segment_of_dots_addresses_that_name_which_kafka_refuses_and_not_the_topic_list() {
    let w = support_write::world("dot-topics", |_, _| {}).await;
    w.create_topic("orders", 1, &[]).await;
    let app = w.router();
    for segment in ["!.", "!..", "%2E", "%2E%2E"] {
        let (s, v) = call(&app, "GET", &format!("/api/v1/topics/{segment}"), "").await;
        // Kafka has no topic named `.` or `..`: the frame says it is missing, it is not the list of topics.
        assert!(v.get("topics").is_none(), "{segment}: {s} {v}");
        assert!(!v.to_string().contains("\"orders\""), "{segment}: {s} {v}");
    }
    w.stop().await;
}
