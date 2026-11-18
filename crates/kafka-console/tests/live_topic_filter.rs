//! CON-850: the Live topic filter, over a real broker and real requests through the recorder. The owner picked a topic on the real
//! Live page and said the filter "does not seem to work". The rows did narrow, but (a) the match was a substring (`orders` kept
//! the rows of `orders.dlq`), and (b) a request that names several topics, or a group's commit, matched while its Target cell
//! named the first topic or the group, so the visible rows did not say `orders` at all. A row that passes a topic filter now says
//! the topic it passed by.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_live;
mod support_write;

use kafka_console::model::{WireQuery, WireRow};
use kafka_console::source::now_ms;
use kafka_wire::messages::metadata_request::MetadataRequestTopic;
use kafka_wire::messages::offset_commit_request::{
    OffsetCommitRequestPartition, OffsetCommitRequestTopic,
};
use kafka_wire::messages::*;
use support_live::*;

fn query(s: &str) -> WireQuery {
    WireQuery::parse(&kafka_console::form::parse(&format!("idle=0&{s}")))
}

fn commit(group: &str, topic: &str) -> OffsetCommitRequest {
    let mut p = OffsetCommitRequestPartition::default();
    p.partition_index = 0;
    p.committed_offset = 1;
    p.committed_leader_epoch = -1;
    let mut t = OffsetCommitRequestTopic::default();
    t.name = TopicName(s(topic));
    t.partitions = vec![p];
    let mut c = OffsetCommitRequest::default();
    c.group_id = GroupId(s(group));
    c.generation_id_or_member_epoch = -1;
    c.topics = vec![t];
    c
}

fn metadata(topics: &[&str]) -> MetadataRequest {
    let mut m = MetadataRequest::default();
    m.topics = Some(
        topics
            .iter()
            .map(|n| {
                let mut t = MetadataRequestTopic::default();
                t.name = Some(TopicName(s(n)));
                t
            })
            .collect(),
    );
    m
}

#[tokio::test]
async fn a_topic_filter_keeps_that_topics_rows_exactly_and_each_row_says_the_topic() {
    let (w, rec) = recorded("topic-filter", false).await;
    for t in ["orders", "orders.dlq", "inventory.levels"] {
        w.create_topic(t, 1, &[]).await;
    }
    let mut c = Wire::connect(&w).await;
    let b = || batch(Some("k"), b"v".to_vec(), None);
    c.call(ApiKey::Produce, 9, "api", &produce("orders", b(), None))
        .await;
    c.call(ApiKey::Produce, 9, "api", &produce("orders.dlq", b(), None))
        .await;
    c.call(ApiKey::Fetch, 12, "api", &fetch("orders")).await;
    c.call(ApiKey::Fetch, 12, "api", &fetch("orders.dlq")).await;
    c.call(
        ApiKey::OffsetCommit,
        8,
        "worker",
        &commit("fulfilment", "orders"),
    )
    .await;
    c.call(
        ApiKey::OffsetCommit,
        8,
        "worker",
        &commit("dlq-reader", "orders.dlq"),
    )
    .await;
    c.call(
        ApiKey::Metadata,
        12,
        "admin",
        &metadata(&["inventory.levels", "orders.dlq", "orders"]),
    )
    .await;
    let tape = |q: &str| rec.tape(&query(q), now_ms()).rows;
    let says = |r: &WireRow, topic: &str| {
        let shown = format!("{} {}", r.target, r.target_sub.as_deref().unwrap_or(""));
        shown.split(|c: char| c.is_whitespace()).any(|word| {
            word == topic
                || word
                    .strip_prefix(topic)
                    .is_some_and(|rest| rest.starts_with('-') && rest[1..].parse::<u32>().is_ok())
        })
    };

    let rows = tape("topic=orders");
    let apis: Vec<_> = rows.iter().map(|r| r.api).collect();
    // Produce, Fetch, the commit to orders and the Metadata that names it: not the dlq's own rows.
    assert_eq!(rows.len(), 4, "{apis:?}");
    assert!(
        rows.iter().all(|r| r.topics.iter().any(|t| t == "orders")),
        "every row names the topic: {rows:?}"
    );
    for r in &rows {
        assert!(
            says(r, "orders"),
            "the row says the topic it passed by: {} {} / {:?} ({})",
            r.api,
            r.target,
            r.target_sub,
            r.client
        );
    }

    // Several topics are an OR: the dlq joins, `inventory.levels` only through Metadata.
    let both = tape("topic=orders&topic=orders.dlq");
    assert_eq!(both.len(), 7);
    // Kinds combine with AND.
    let produce_only = tape("topic=orders&topic=orders.dlq&api=Produce");
    assert_eq!(produce_only.len(), 2);
    let one_client = tape("topic=orders&client=worker");
    assert_eq!(one_client.len(), 1);
    assert_eq!(one_client[0].api, "OffsetCommit");
    let two_clients = tape("topic=orders&client=worker&client=admin");
    assert_eq!(two_clients.len(), 2);
    w.stop().await;
}
