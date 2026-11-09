//! Every version of the transaction APIs the broker serves answers in a form that can be written (see
//! `versions_data.rs` for why that is the whole test: the rig encodes each response at the version asked).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_wire::api::Request;
use kafka_wire::messages::add_partitions_to_txn_request::AddPartitionsToTxnTopic;
use kafka_wire::messages::describe_producers_request::TopicRequest;
use kafka_wire::messages::txn_offset_commit_request::{
    TxnOffsetCommitRequestPartition, TxnOffsetCommitRequestTopic,
};
use kafka_wire::messages::{
    AddOffsetsToTxnRequest, AddPartitionsToTxnRequest, DescribeProducersRequest,
    DescribeTransactionsRequest, EndTxnRequest, GroupId, InitProducerIdRequest,
    ListTransactionsRequest, ProducerId, TransactionalId, TxnOffsetCommitRequest,
};
use support::Txn;
use support::{rig, topic, wire_name};

fn text(s: &str) -> kafka_wire::StrBytes {
    kafka_wire::StrBytes::from_string(s.to_owned())
}

/// One producer's transaction through each API at version `v`: a producer for each version, so that a transaction left
/// open by one does not decide the next.
async fn one_transaction_at(rig: &support::Rig, v: i16) {
    let id = format!("v-{v}");
    let Txn { producer, epoch } = rig.init_txn(&id, 60_000).await.unwrap();
    let mut add = AddPartitionsToTxnRequest::default();
    add.v3_and_below_transactional_id = TransactionalId(text(&id));
    add.v3_and_below_producer_id = ProducerId(producer);
    add.v3_and_below_producer_epoch = epoch;
    let mut t = AddPartitionsToTxnTopic::default();
    t.name = wire_name("t");
    t.partitions = vec![0, 1, 9];
    add.v3_and_below_topics.push(t);
    rig.ask(v, Request::AddPartitionsToTxn(add.clone())).await;
    // Again for a topic that is not there, and as a producer that is not the one.
    add.v3_and_below_topics[0].name = wire_name("nope");
    rig.ask(v, Request::AddPartitionsToTxn(add.clone())).await;
    add.v3_and_below_producer_epoch = epoch + 1;
    rig.ask(v, Request::AddPartitionsToTxn(add)).await;

    let mut offsets = AddOffsetsToTxnRequest::default();
    offsets.transactional_id = TransactionalId(text(&id));
    offsets.producer_id = ProducerId(producer);
    offsets.producer_epoch = epoch;
    offsets.group_id = GroupId(text("g"));
    rig.ask(v, Request::AddOffsetsToTxn(offsets.clone())).await;
    offsets.producer_epoch = epoch + 1;
    rig.ask(v, Request::AddOffsetsToTxn(offsets)).await;

    let mut commit = TxnOffsetCommitRequest::default();
    commit.transactional_id = TransactionalId(text(&id));
    commit.group_id = GroupId(text("g"));
    commit.producer_id = ProducerId(producer);
    commit.producer_epoch = epoch;
    commit.generation_id = -1;
    let mut p = TxnOffsetCommitRequestPartition::default();
    p.partition_index = 0;
    p.committed_offset = 3;
    p.committed_leader_epoch = -1;
    let mut t = TxnOffsetCommitRequestTopic::default();
    t.name = wire_name("t");
    t.partitions.push(p);
    commit.topics.push(t);
    rig.ask(v, Request::TxnOffsetCommit(commit.clone())).await;
    commit.topics[0].name = wire_name("nope");
    rig.ask(v, Request::TxnOffsetCommit(commit)).await;

    // Ended twice: the answer to the first, and to the one that finds nothing to end.
    for committed in [v % 2 == 0, true] {
        let mut end = EndTxnRequest::default();
        end.transactional_id = TransactionalId(text(&id));
        end.producer_id = ProducerId(producer);
        end.producer_epoch = epoch;
        end.committed = committed;
        rig.ask(v, Request::EndTxn(end)).await;
    }
}

#[tokio::test]
async fn a_transaction_is_run_through_every_version_of_each_api() {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    for v in 0..=4 {
        let mut req = InitProducerIdRequest::default();
        req.transactional_id = Some(TransactionalId(text(&format!("tx-{v}"))));
        req.transaction_timeout_ms = 60_000;
        rig.ask(v, Request::InitProducerId(req)).await;
    }
    for v in 0..=3 {
        one_transaction_at(&rig, v).await;
    }
    let mut ask = DescribeProducersRequest::default();
    let mut t = TopicRequest::default();
    t.name = wire_name("t");
    t.partition_indexes = vec![0, 1, 9];
    ask.topics.push(t);
    rig.ask(0, Request::DescribeProducers(ask)).await;
    let mut ask = DescribeTransactionsRequest::default();
    ask.transactional_ids = vec![TransactionalId(text("v-0")), TransactionalId(text("nope"))];
    rig.ask(0, Request::DescribeTransactions(ask)).await;
    for v in 0..=1 {
        rig.ask(
            v,
            Request::ListTransactions(ListTransactionsRequest::default()),
        )
        .await;
    }
}
