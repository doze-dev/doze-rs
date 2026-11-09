//! A transactional producer and the things that change its transaction, at the same time.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::sync::Arc;
use std::time::Duration;

use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::{ProduceRequest, TransactionalId};
use support::{Txn, records, rig, topic, wire_name};

/// A Produce of one transactional batch to each of partitions 0 and 1 of `t`.
fn two_partitions(t: Txn) -> ProduceRequest {
    let mut data = TopicProduceData::default();
    data.name = wire_name("t");
    for index in 0..2 {
        let batch = kafka_wire::batch::build::BatchBuilder::new()
            .records(1)
            .attributes(0x10)
            .timestamps(1_000, 1_000)
            .producer(t.producer, t.epoch, 0)
            .build();
        let mut p = PartitionProduceData::default();
        p.index = index;
        p.records = Some(batch);
        data.partition_data.push(p);
    }
    let mut req = ProduceRequest::default();
    req.acks = 1;
    req.timeout_ms = 5_000;
    req.topic_data.push(data);
    req.transactional_id = Some(TransactionalId(StrBytes::from_string("tx".into())));
    req
}

/// The transaction is held for the whole of a Produce request, not once for each partition: a hold waited for while another
/// is kept stands behind a writer that is itself waiting for the first. A new owner of the id (which writes the
/// transaction) arrives while a request for two partitions has queued the first and waits to queue the second.
#[tokio::test]
async fn a_new_owner_arriving_while_a_produce_spans_two_partitions_neither_waits_for_ever_nor_is_waited_for()
 {
    let rig = Arc::new(rig());
    rig.create(vec![topic("t", 2)], false).await;
    let t = rig.init_txn("tx", 60_000).await.unwrap();
    assert!(
        rig.add_partitions("tx", t, "t", &[0, 1])
            .await
            .iter()
            .all(|c| *c == 0)
    );
    // Partition 0's queue is full of other producers' batches, so the request suspends after queueing its batch for
    // partition 1... and, before the repair, between taking the hold for partition 0 and for partition 1.
    let mut others = Vec::new();
    for _ in 0..40 {
        let r = Arc::clone(&rig);
        others.push(tokio::spawn(async move {
            r.produce(1, "t", 0, records(1)).await;
        }));
    }
    let r = Arc::clone(&rig);
    let produce = tokio::spawn(async move { r.ask(9, Request::Produce(two_partitions(t))).await });
    let r = Arc::clone(&rig);
    let owner = tokio::spawn(async move { r.init_txn("tx", 60_000).await });
    let done = async { (produce.await, owner.await) };
    let (produced, owned) = tokio::time::timeout(Duration::from_secs(5), done)
        .await
        .expect("the produce and the new owner both finish");
    let Response::Produce(p) = produced.unwrap() else {
        panic!("not Produce")
    };
    assert_eq!(p.responses[0].partition_responses.len(), 2);
    assert!(owned.unwrap().is_ok());
    for other in others {
        other.await.unwrap();
    }
}

/// A transaction cannot be ended in the middle of a Produce request that has begun to write to it: the request holds it,
/// from its first check until its last batch is queued, so a batch checked against the open transaction is never written
/// behind its marker (which would hold the last stable offset still for ever). Here EndTxn arrives while the request waits
/// for room in partition 0 and has not yet looked at partition 1.
#[tokio::test]
async fn a_transaction_is_not_ended_in_the_middle_of_a_produce_request_that_writes_to_it() {
    let rig = Arc::new(rig());
    rig.create(vec![topic("t", 2)], false).await;
    let t = rig.init_txn("tx", 60_000).await.unwrap();
    assert!(
        rig.add_partitions("tx", t, "t", &[0, 1])
            .await
            .iter()
            .all(|c| *c == 0)
    );
    let mut others = Vec::new();
    for _ in 0..40 {
        let r = Arc::clone(&rig);
        others.push(tokio::spawn(async move {
            r.produce(1, "t", 0, records(1)).await;
        }));
    }
    let r = Arc::clone(&rig);
    let produce = tokio::spawn(async move { r.ask(9, Request::Produce(two_partitions(t))).await });
    let r = Arc::clone(&rig);
    let end = tokio::spawn(async move { r.end_txn("tx", t, true).await });
    let Response::Produce(p) = produce.await.unwrap() else {
        panic!("not Produce")
    };
    assert_eq!(
        p.responses[0]
            .partition_responses
            .iter()
            .map(|r| r.error_code)
            .collect::<Vec<_>>(),
        [0, 0],
        "both batches were checked against the open transaction"
    );
    assert_eq!(end.await.unwrap(), 0);
    for other in others {
        other.await.unwrap();
    }
    for partition in 0..2 {
        let got = rig.fetch_committed("t", partition, 0).await;
        assert_eq!(got.last_stable_offset, got.high_watermark);
    }
}
