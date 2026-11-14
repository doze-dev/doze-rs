//! Shared by the planted-secret gate (`tests/redaction.rs`): a JSON POST as the app sends it, the first frames of a tail, and
//! a group's offset commit.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use kafka_wire::StrBytes;
use kafka_wire::messages::{GroupId, OffsetCommitRequest, TopicName};

/// A POST of JSON, as the React app's fetch client sends it.
pub fn json_post(path: &str, body: &serde_json::Value) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method("POST")
        .uri(path)
        .header(axum::http::header::HOST, crate::support::HOST)
        .header("x-console", "1")
        .header("sec-fetch-site", "same-origin")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_string()))
        .unwrap()
}

/// A tail from the first cursor of this process (`Last-Event-ID`): what it sends in its first second.
pub async fn first_frames(app: &axum::Router, path: &str) -> String {
    use http_body_util::BodyExt;
    let mut req = crate::support::api_get(path);
    req.headers_mut().insert(
        "last-event-id",
        kafka_console::api::live::cursor(0).parse().unwrap(),
    );
    let res = crate::support::head(app, req).await;
    assert_eq!(res.status(), axum::http::StatusCode::OK, "{path}");
    let mut body = res.into_body();
    let mut out = String::new();
    let until = tokio::time::Instant::now() + std::time::Duration::from_millis(1500);
    while let Ok(Some(Ok(frame))) = tokio::time::timeout_at(until, body.frame()).await {
        if let Some(data) = frame.data_ref() {
            out.push_str(&String::from_utf8_lossy(data));
        }
    }
    out
}

pub fn commit(group: &str, topic: &str) -> OffsetCommitRequest {
    use kafka_wire::messages::offset_commit_request::{
        OffsetCommitRequestPartition, OffsetCommitRequestTopic,
    };
    let mut p = OffsetCommitRequestPartition::default();
    p.partition_index = 0;
    p.committed_offset = 0;
    p.committed_leader_epoch = -1;
    let mut t = OffsetCommitRequestTopic::default();
    t.name = TopicName(StrBytes::from_string(topic.to_owned()));
    t.partitions = vec![p];
    let mut c = OffsetCommitRequest::default();
    c.group_id = GroupId(StrBytes::from_string(group.to_owned()));
    c.generation_id_or_member_epoch = -1;
    c.topics = vec![t];
    c
}
