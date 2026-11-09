//! The codes the transaction APIs answer with, and the order they are decided in (read from Apache Kafka 4.3.1's source:
//! the recordings hold only the refusals of `txn-empty-id` and `endtxn-decisions`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::add_partitions_to_txn_request::AddPartitionsToTxnTopic;
use kafka_wire::messages::{
    AddOffsetsToTxnRequest, AddPartitionsToTxnRequest, EndTxnRequest, GroupId, ProducerId,
    TransactionalId,
};
use support::{Rig, Txn, rig, topic, wire_name};

const INVALID_PRODUCER_EPOCH: i16 = 47;
const PRODUCER_FENCED: i16 = 90;
const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const OPERATION_NOT_ATTEMPTED: i16 = 55;
const INVALID_PRODUCER_ID_MAPPING: i16 = 49;

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

async fn started() -> (Rig, Txn) {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    let t = rig.init_txn("tx", 60_000).await.unwrap();
    (rig, t)
}

async fn end_at(rig: &Rig, version: i16, t: Txn, epoch: i16) -> i16 {
    let mut req = EndTxnRequest::default();
    req.transactional_id = TransactionalId(text("tx"));
    req.producer_id = ProducerId(t.producer);
    req.producer_epoch = epoch;
    req.committed = true;
    let Response::EndTxn(r) = rig.ask(version, Request::EndTxn(req)).await else {
        panic!("not EndTxn")
    };
    r.error_code
}

#[tokio::test]
async fn an_epoch_that_is_not_the_owners_is_fenced_and_a_version_without_that_code_is_told_the_epoch_is_invalid()
 {
    let (rig, t) = started().await;
    for wrong in [t.epoch - 1, t.epoch + 1] {
        assert_eq!(end_at(&rig, 3, t, wrong).await, PRODUCER_FENCED);
        assert_eq!(end_at(&rig, 2, t, wrong).await, PRODUCER_FENCED);
        assert_eq!(end_at(&rig, 1, t, wrong).await, INVALID_PRODUCER_EPOCH);
        let mut add = AddOffsetsToTxnRequest::default();
        add.transactional_id = TransactionalId(text("tx"));
        add.producer_id = ProducerId(t.producer);
        add.producer_epoch = wrong;
        add.group_id = GroupId(text("g"));
        for (version, want) in [(1, INVALID_PRODUCER_EPOCH), (2, PRODUCER_FENCED)] {
            let Response::AddOffsetsToTxn(r) = rig
                .ask(version, Request::AddOffsetsToTxn(add.clone()))
                .await
            else {
                panic!("not AddOffsetsToTxn")
            };
            assert_eq!(r.error_code, want, "v{version}");
        }
        let stranger = Txn { epoch: wrong, ..t };
        assert_eq!(
            rig.add_partitions("tx", stranger, "t", &[0]).await,
            [PRODUCER_FENCED]
        );
    }
    let other = Txn {
        producer: t.producer + 1,
        ..t
    };
    assert_eq!(
        end_at(&rig, 3, other, t.epoch).await,
        INVALID_PRODUCER_ID_MAPPING
    );
}

#[tokio::test]
async fn partitions_are_judged_before_the_transaction_is_looked_at() {
    let (rig, t) = started().await;
    let mut req = AddPartitionsToTxnRequest::default();
    // Nobody owns this id, and one of the partitions is not there: the partition is what is answered, the rest are not
    // attempted, and the coordinator is never asked.
    req.v3_and_below_transactional_id = TransactionalId(text("nobody"));
    req.v3_and_below_producer_id = ProducerId(t.producer);
    req.v3_and_below_producer_epoch = t.epoch;
    let mut topic = AddPartitionsToTxnTopic::default();
    topic.name = wire_name("t");
    topic.partitions = vec![0, 7];
    req.v3_and_below_topics.push(topic);
    let Response::AddPartitionsToTxn(r) =
        rig.ask(3, Request::AddPartitionsToTxn(req.clone())).await
    else {
        panic!("not AddPartitionsToTxn")
    };
    let codes: Vec<i16> = r.results_by_topic_v3_and_below[0]
        .results_by_partition
        .iter()
        .map(|p| p.partition_error_code)
        .collect();
    assert_eq!(codes, [OPERATION_NOT_ATTEMPTED, UNKNOWN_TOPIC_OR_PARTITION]);
    // With every partition there, the coordinator is asked, and answers for the whole request.
    req.v3_and_below_topics[0].partitions = vec![0, 1];
    let Response::AddPartitionsToTxn(r) = rig.ask(3, Request::AddPartitionsToTxn(req)).await else {
        panic!("not AddPartitionsToTxn")
    };
    assert!(
        r.results_by_topic_v3_and_below[0]
            .results_by_partition
            .iter()
            .all(|p| p.partition_error_code == INVALID_PRODUCER_ID_MAPPING)
    );
}

#[tokio::test]
async fn a_transaction_times_out_from_when_it_began_whatever_is_added_to_it_since() {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    let t = rig.init_txn("tx", 1_000).await.unwrap();
    assert_eq!(rig.add_partitions("tx", t, "t", &[0]).await, [0]);
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(rig.add_partitions("tx", t, "t", &[1]).await, [0]);
    tokio::time::sleep(Duration::from_millis(500)).await;
    rig.broker.check_transactions().await;
    assert_eq!(
        end_at(&rig, 3, t, t.epoch).await,
        PRODUCER_FENCED,
        "aborted by the broker 1.2 s after it began, though a partition was added at 0.7 s"
    );
}

async fn init_with(rig: &Rig, version: i16, pair: (i64, i16)) -> (i16, i64, i16) {
    let mut req = kafka_wire::messages::InitProducerIdRequest::default();
    req.transactional_id = Some(TransactionalId(text("tx")));
    req.transaction_timeout_ms = 60_000;
    req.producer_id = ProducerId(pair.0);
    req.producer_epoch = pair.1;
    let Response::InitProducerId(r) = rig.ask(version, Request::InitProducerId(req)).await else {
        panic!("not InitProducerId")
    };
    (r.error_code, r.producer_id.0, r.producer_epoch)
}

#[tokio::test]
async fn a_producer_that_says_what_it_holds_is_bumped_asked_again_it_is_answered_alike_and_a_stranger_is_fenced()
 {
    const INVALID_REQUEST: i16 = 42;
    let (rig, t) = started().await;
    // The owner bumps itself.
    assert_eq!(
        init_with(&rig, 4, (t.producer, t.epoch)).await,
        (0, t.producer, t.epoch + 1)
    );
    // The answer was lost and it asks again with what it had: the same answer, not another bump.
    assert_eq!(
        init_with(&rig, 4, (t.producer, t.epoch)).await,
        (0, t.producer, t.epoch + 1)
    );
    // A producer at an epoch that has gone, or of another id, is fenced (version 3 has no such code).
    assert_eq!(
        init_with(&rig, 4, (t.producer, t.epoch + 5)).await.0,
        PRODUCER_FENCED
    );
    assert_eq!(
        init_with(&rig, 3, (t.producer, t.epoch + 5)).await.0,
        INVALID_PRODUCER_EPOCH
    );
    assert_eq!(
        init_with(&rig, 4, (t.producer + 1, t.epoch + 1)).await.0,
        PRODUCER_FENCED
    );
    // A new owner (it says nothing) takes the id; the old one, holding what it held, is fenced and cannot take it back.
    let new = init_with(&rig, 4, (-1, -1)).await;
    assert_eq!(new, (0, t.producer, t.epoch + 2));
    assert_eq!(
        init_with(&rig, 4, (t.producer, t.epoch + 1)).await.0,
        PRODUCER_FENCED
    );
    // Half a pair is not one.
    assert_eq!(
        init_with(&rig, 4, (t.producer, -1)).await.0,
        INVALID_REQUEST
    );
    assert_eq!(init_with(&rig, 4, (-1, 2)).await.0, INVALID_REQUEST);
}
