//! The fourteen boards of the topic writes, rendered from a real broker in this process and written to
//! `target/board-diff-live/<id>.html` for `tools/board-diff.ts` (structure only: the same columns, buttons, links and
//! cards as the board, whatever the names and numbers).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_write;

use std::path::PathBuf;

use axum::http::StatusCode;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::add_partitions_to_txn_request::AddPartitionsToTxnTopic;
use kafka_wire::messages::offset_commit_request::{
    OffsetCommitRequestPartition, OffsetCommitRequestTopic,
};
use kafka_wire::messages::{
    AddPartitionsToTxnRequest, GroupId, InitProducerIdRequest, OffsetCommitRequest, ProducerId,
    TopicName, TransactionalId,
};
use support::get;
use support_write::{World, page, post_form, world};

fn out_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap().parent().unwrap();
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    target.join("board-diff-live")
}

/// A group that has committed offset `offset` on partition 0 of `topic`, and an open transaction on `topic`-0.
async fn seed_use(w: &World, topic: &str) {
    let mut p = OffsetCommitRequestPartition::default();
    p.partition_index = 0;
    p.committed_offset = 2;
    p.committed_leader_epoch = -1;
    let mut t = OffsetCommitRequestTopic::default();
    t.name = TopicName(StrBytes::from_string(topic.to_owned()));
    t.partitions = vec![p];
    let mut c = OffsetCommitRequest::default();
    c.group_id = GroupId(StrBytes::from_static_str("payments-workers"));
    c.generation_id_or_member_epoch = -1;
    c.topics = vec![t];
    let Response::OffsetCommit(r) = w.ask(8, Request::OffsetCommit(c)).await else {
        panic!()
    };
    assert_eq!(r.topics[0].partitions[0].error_code, 0, "{r:?}");
    let mut init = InitProducerIdRequest::default();
    init.transactional_id = Some(TransactionalId(StrBytes::from_static_str("orders-eos")));
    init.transaction_timeout_ms = 60_000;
    let Response::InitProducerId(id) = w.ask(4, Request::InitProducerId(init)).await else {
        panic!()
    };
    let mut add = AddPartitionsToTxnRequest::default();
    add.v3_and_below_transactional_id = TransactionalId(StrBytes::from_static_str("orders-eos"));
    add.v3_and_below_producer_id = ProducerId(id.producer_id.0);
    add.v3_and_below_producer_epoch = id.producer_epoch;
    let mut tp = AddPartitionsToTxnTopic::default();
    tp.name = TopicName(StrBytes::from_string(topic.to_owned()));
    tp.partitions = vec![0];
    add.v3_and_below_topics.push(tp);
    let Response::AddPartitionsToTxn(r) = w.ask(3, Request::AddPartitionsToTxn(add)).await else {
        panic!()
    };
    assert_eq!(
        r.results_by_topic_v3_and_below[0].results_by_partition[0].partition_error_code,
        0
    );
}

#[tokio::test]
async fn the_fourteen_boards_are_rendered_from_a_real_broker() {
    let w = world("boards", |_, _| {}).await;
    w.create_topic("orders", 3, &[("retention.ms", "604800000")])
        .await;
    w.create_topic("inventory.levels", 1, &[("cleanup.policy", "compact")])
        .await;
    w.create_topic("orders.dlq", 1, &[]).await;
    seed_use(&w, "orders").await;
    let app = w.router();
    let dir = out_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let json = r#"{"order_id":"ord-88420","customer":"c-3092","items":[{"sku":"FV-TOM-V","qty":1}],"total":7.40}"#;
    let order = [
        ("partition", "2"),
        ("key", "ord-88420"),
        ("value", json),
        ("hn0", "content-type"),
        ("hv0", "application/json"),
        ("acks", "-1"),
    ];
    let new = |name: &'static str, mode: &'static str| {
        vec![
            ("name", name),
            ("partitions", "3"),
            ("template", "event"),
            ("cleanup.policy", "delete"),
            ("retention.ms", "604800000"),
            ("mode", mode),
        ]
    };
    let q = "name=orders.retry&partitions=3&template=event&cleanup.policy=delete&retention.ms=604800000";
    let boards: Vec<(&str, Option<(String, Vec<(&str, &str)>)>, String)> = vec![
        ("topic-create", None, format!("/topics/new?step=name&{q}")),
        ("topic-create-config", None, format!("/topics/new?step=config&{q}")),
        ("topic-create-review", None, format!("/topics/new?step=review&{q}")),
        ("topic-create-refused", Some(("/topics/new".to_owned(), new("orders", "create"))), String::new()),
        ("topic-created", Some(("/topics/new".to_owned(), new("orders.retry", "create"))), String::new()),
        ("topic-produce", None, "/topics/orders/produce?partition=2&key=ord-88420&hn0=content-type&hv0=application%2Fjson&acks=-1".to_owned()),
        ("topic-produce-keyless", Some(("/topics/inventory.levels/produce".to_owned(), vec![("key", ""), ("value", r#"{"sku":"FV-TOM-V","on_hand":412}"#), ("acks", "-1")])), String::new()),
        ("topic-produced", Some(("/topics/orders/produce".to_owned(), order.to_vec())), String::new()),
        ("topic-add-partitions", None, "/topics/orders/partitions/add?count=5".to_owned()),
        ("topic-add-partitions-refused", Some(("/topics/orders/partitions/add".to_owned(), vec![("count", "2")])), String::new()),
        ("topic-delete-records", None, "/topics/orders/delete-records?partition=0&offset=1".to_owned()),
        ("topic-delete-records-refused", Some(("/topics/inventory.levels/delete-records".to_owned(), vec![("partition", "0"), ("offset", "0")])), String::new()),
        ("topic-delete", None, "/topics/orders/delete".to_owned()),
        ("topic-deleted", Some(("/topics/orders/delete".to_owned(), vec![("confirm", "orders")])), String::new()),
    ];
    for (id, post, path) in boards {
        let req = match post {
            Some((p, fields)) => post_form(&p, &fields),
            None => get(&path),
        };
        let (status, doc) = page(&app, req).await;
        assert_eq!(status, StatusCode::OK, "{id}: {doc:.300}");
        std::fs::write(dir.join(format!("{id}.html")), &doc).unwrap();
    }
    w.stop().await;
}
