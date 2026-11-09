//! What DescribeTransactions, ListTransactions and DescribeProducers say about a transaction.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::describe_producers_request::TopicRequest;
use kafka_wire::messages::{
    DescribeProducersRequest, DescribeTransactionsRequest, ListTransactionsRequest, ProducerId,
    TransactionalId,
};
use support::{Rig, Txn, rig, topic, wire_name};

const TRANSACTIONAL_ID_NOT_FOUND: i16 = 105;

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

async fn describe(
    rig: &Rig,
    id: &str,
) -> kafka_wire::messages::describe_transactions_response::TransactionState {
    let mut req = DescribeTransactionsRequest::default();
    req.transactional_ids = vec![TransactionalId(text(id))];
    let Response::DescribeTransactions(mut r) =
        rig.ask(0, Request::DescribeTransactions(req)).await
    else {
        panic!("not DescribeTransactions")
    };
    r.transaction_states.remove(0)
}

async fn listed(
    rig: &Rig,
    version: i16,
    states: &[&str],
    producers: &[i64],
    longer_than: i64,
) -> Vec<String> {
    let mut req = ListTransactionsRequest::default();
    req.state_filters = states.iter().map(|s| text(s)).collect();
    req.producer_id_filters = producers.iter().map(|p| ProducerId(*p)).collect();
    req.duration_filter = longer_than;
    let Response::ListTransactions(r) = rig.ask(version, Request::ListTransactions(req)).await
    else {
        panic!("not ListTransactions")
    };
    r.transaction_states
        .iter()
        .map(|s| {
            format!(
                "{}:{}",
                s.transactional_id.0.as_str(),
                s.transaction_state.as_str()
            )
        })
        .collect()
}

async fn two_transactions() -> (Rig, Txn, Txn) {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    let a = rig.init_txn("a", 60_000).await.unwrap();
    rig.add_partitions("a", a, "t", &[0, 1]).await;
    rig.produce_txn("a", a, ("t", 0), 0, 2).await;
    rig.add_offsets("a", a, "g").await;
    let b = rig.init_txn("b", 30_000).await.unwrap();
    rig.add_partitions("b", b, "t", &[1]).await;
    rig.end_txn("b", b, true).await;
    (rig, a, b)
}

#[tokio::test]
async fn a_transaction_is_described_while_it_is_open_and_after() {
    let (rig, a, b) = two_transactions().await;
    let open = describe(&rig, "a").await;
    assert_eq!(open.error_code, 0);
    assert_eq!(open.transaction_state.as_str(), "Ongoing");
    assert_eq!(
        (
            open.producer_id.0,
            open.producer_epoch,
            open.transaction_timeout_ms
        ),
        (a.producer, a.epoch, 60_000)
    );
    assert!(open.transaction_start_time_ms > 0);
    let topics: Vec<(String, Vec<i32>)> = open
        .topics
        .iter()
        .map(|t| (t.topic.0.to_string(), t.partitions.clone()))
        .collect();
    assert_eq!(
        topics[0],
        ("__consumer_offsets".to_owned(), vec![kafka_partition("g")])
    );
    assert_eq!(topics[1], ("t".to_owned(), vec![0, 1]));
    let done = describe(&rig, "b").await;
    assert_eq!(done.transaction_state.as_str(), "CompleteCommit");
    assert_eq!(done.transaction_start_time_ms, -1, "nothing is open");
    assert!(done.topics.is_empty());
    assert_eq!(done.producer_id.0, b.producer);
    assert_eq!(
        describe(&rig, "nobody").await.error_code,
        TRANSACTIONAL_ID_NOT_FOUND
    );
}

/// Kafka's partition of the offsets topic for a group (`abs(hashCode) % 50`).
fn kafka_partition(group: &str) -> i32 {
    let mut h: i32 = 0;
    for c in group.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(i32::from(c));
    }
    (if h == i32::MIN { 0 } else { h.abs() }) % 50
}

#[tokio::test]
async fn transactions_are_listed_by_state_producer_and_how_long_they_have_run() {
    let (rig, a, b) = two_transactions().await;
    assert_eq!(
        listed(&rig, 0, &[], &[], -1).await,
        ["a:Ongoing", "b:CompleteCommit"]
    );
    assert_eq!(listed(&rig, 0, &["Ongoing"], &[], -1).await, ["a:Ongoing"]);
    assert_eq!(
        listed(&rig, 0, &["CompleteCommit", "Ongoing"], &[], -1)
            .await
            .len(),
        2
    );
    assert_eq!(
        listed(&rig, 0, &["PrepareAbort"], &[], -1).await,
        Vec::<String>::new()
    );
    assert_eq!(
        listed(&rig, 0, &[], &[b.producer], -1).await,
        ["b:CompleteCommit"]
    );
    assert_eq!(
        listed(&rig, 0, &[], &[a.producer, b.producer], -1)
            .await
            .len(),
        2
    );
    // Version 1: those that have run for longer than so many milliseconds are the open ones.
    assert_eq!(listed(&rig, 1, &[], &[], 0).await, ["a:Ongoing"]);
    assert_eq!(
        listed(&rig, 1, &[], &[], 3_600_000).await,
        Vec::<String>::new()
    );
}

#[tokio::test]
async fn the_producers_of_a_partition_are_listed_with_where_their_open_transaction_began() {
    let (rig, a, _) = two_transactions().await;
    let ask = |partitions: Vec<i32>| {
        let rig = &rig;
        async move {
            let mut t = TopicRequest::default();
            t.name = wire_name("t");
            t.partition_indexes = partitions;
            let mut req = DescribeProducersRequest::default();
            req.topics.push(t);
            let Response::DescribeProducers(r) = rig.ask(0, Request::DescribeProducers(req)).await
            else {
                panic!("not DescribeProducers")
            };
            r.topics[0].partitions.clone()
        }
    };
    let got = ask(vec![0, 9]).await;
    assert_eq!(got[0].error_code, 0);
    let producer = &got[0].active_producers[0];
    assert_eq!(
        (
            producer.producer_id.0,
            producer.producer_epoch,
            producer.last_sequence,
            producer.current_txn_start_offset
        ),
        (a.producer, i32::from(a.epoch), 1, 0)
    );
    assert_eq!(got[1].error_code, 3, "no such partition");
    assert_eq!(rig.end_txn("a", a, true).await, 0);
    let after = ask(vec![0]).await;
    assert_eq!(after[0].active_producers[0].current_txn_start_offset, -1);
}

#[tokio::test]
async fn a_fenced_producers_epoch_is_raised_on_the_partition_by_the_marker_that_aborted_it() {
    let (rig, a, _) = two_transactions().await;
    // A new owner of "a" aborts what the old one left, with markers at the next epoch.
    let new = rig.init_txn("a", 60_000).await.unwrap();
    // (The abort raises the epoch once, for its markers; taking the id over raises it again.)
    assert_eq!(new.epoch, a.epoch + 2);
    let mut t = TopicRequest::default();
    t.name = wire_name("t");
    t.partition_indexes = vec![0];
    let mut req = DescribeProducersRequest::default();
    req.topics.push(t);
    let Response::DescribeProducers(r) = rig.ask(0, Request::DescribeProducers(req)).await else {
        panic!("not DescribeProducers")
    };
    let producer = &r.topics[0].partitions[0].active_producers[0];
    assert_eq!(
        (
            producer.producer_id.0,
            producer.producer_epoch,
            producer.current_txn_start_offset
        ),
        (a.producer, i32::from(a.epoch + 1), -1)
    );
}
