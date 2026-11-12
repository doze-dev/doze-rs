//! The transaction APIs put to a broker as a given caller, for the authorization tests.
#![allow(
    unreachable_pub,
    dead_code,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::gx::{group_id, text, topic_name};
use crate::support::{Rig, Txn};
use kafka_broker::Caller;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::add_partitions_to_txn_request::AddPartitionsToTxnTopic;
use kafka_wire::messages::describe_transactions_response::TransactionState;
use kafka_wire::messages::txn_offset_commit_request::{
    TxnOffsetCommitRequestPartition, TxnOffsetCommitRequestTopic,
};
use kafka_wire::messages::{
    AddOffsetsToTxnRequest, AddPartitionsToTxnRequest, AddPartitionsToTxnResponse,
    DescribeTransactionsRequest, EndTxnRequest, InitProducerIdRequest, ListTransactionsRequest,
    ProducerId, TransactionalId, TxnOffsetCommitRequest, TxnOffsetCommitResponse,
};

/// A producer that nobody gave the id to (any answer but a refusal will do).
pub const STRANGER: Txn = Txn {
    producer: 1,
    epoch: 0,
};

pub fn txn_id(s: &str) -> TransactionalId {
    TransactionalId(text(s))
}

/// InitProducerId (v4) for a transactional id: the producer it is to use, or the code.
pub async fn init_txn(rig: &Rig, who: &Caller, id: &str) -> Result<Txn, i16> {
    let mut req = InitProducerIdRequest::default();
    req.transactional_id = Some(txn_id(id));
    req.transaction_timeout_ms = 60_000;
    let Response::InitProducerId(r) = rig.ask_as(who, 4, Request::InitProducerId(req)).await else {
        panic!("not InitProducerId")
    };
    if r.error_code == 0 {
        Ok(Txn {
            producer: r.producer_id.0,
            epoch: r.producer_epoch,
        })
    } else {
        Err(r.error_code)
    }
}

pub async fn add_partitions(
    rig: &Rig,
    who: &Caller,
    id: &str,
    t: Txn,
    topics: &[(&str, &[i32])],
) -> AddPartitionsToTxnResponse {
    let mut req = AddPartitionsToTxnRequest::default();
    req.v3_and_below_transactional_id = txn_id(id);
    req.v3_and_below_producer_id = ProducerId(t.producer);
    req.v3_and_below_producer_epoch = t.epoch;
    req.v3_and_below_topics = topics
        .iter()
        .map(|(n, ps)| {
            let mut topic = AddPartitionsToTxnTopic::default();
            topic.name = topic_name(n);
            topic.partitions = ps.to_vec();
            topic
        })
        .collect();
    match rig.ask_as(who, 3, Request::AddPartitionsToTxn(req)).await {
        Response::AddPartitionsToTxn(r) => r,
        other => panic!("not AddPartitionsToTxn: {other:?}"),
    }
}

/// `(topic, partition, code)` of each partition answered, in order.
pub fn partition_codes(r: &AddPartitionsToTxnResponse) -> Vec<(String, i32, i16)> {
    r.results_by_topic_v3_and_below
        .iter()
        .flat_map(|t| {
            t.results_by_partition.iter().map(|p| {
                (
                    t.name.to_string(),
                    p.partition_index,
                    p.partition_error_code,
                )
            })
        })
        .collect()
}

pub async fn add_offsets(rig: &Rig, who: &Caller, id: &str, t: Txn, group: &str) -> i16 {
    let mut req = AddOffsetsToTxnRequest::default();
    req.transactional_id = txn_id(id);
    req.producer_id = ProducerId(t.producer);
    req.producer_epoch = t.epoch;
    req.group_id = group_id(group);
    let Response::AddOffsetsToTxn(r) = rig.ask_as(who, 3, Request::AddOffsetsToTxn(req)).await
    else {
        panic!("not AddOffsetsToTxn")
    };
    r.error_code
}

pub async fn txn_offset_commit(
    rig: &Rig,
    who: &Caller,
    id: &str,
    t: Txn,
    group: &str,
    topics: &[(&str, &[i32])],
) -> TxnOffsetCommitResponse {
    let mut req = TxnOffsetCommitRequest::default();
    req.transactional_id = txn_id(id);
    req.group_id = group_id(group);
    req.producer_id = ProducerId(t.producer);
    req.producer_epoch = t.epoch;
    req.generation_id = -1;
    req.topics = topics
        .iter()
        .map(|(n, ps)| {
            let mut topic = TxnOffsetCommitRequestTopic::default();
            topic.name = topic_name(n);
            topic.partitions = ps
                .iter()
                .map(|p| {
                    let mut x = TxnOffsetCommitRequestPartition::default();
                    x.partition_index = *p;
                    x.committed_offset = 5;
                    x.committed_leader_epoch = -1;
                    x
                })
                .collect();
            topic
        })
        .collect();
    match rig.ask_as(who, 3, Request::TxnOffsetCommit(req)).await {
        Response::TxnOffsetCommit(r) => r,
        other => panic!("not TxnOffsetCommit: {other:?}"),
    }
}

/// `(topic, partition, code)` of each partition answered, in order.
pub fn offset_codes(r: &TxnOffsetCommitResponse) -> Vec<(String, i32, i16)> {
    r.topics
        .iter()
        .flat_map(|t| {
            t.partitions
                .iter()
                .map(|p| (t.name.to_string(), p.partition_index, p.error_code))
        })
        .collect()
}

pub async fn end_txn(rig: &Rig, who: &Caller, version: i16, id: &str, t: Txn, commit: bool) -> i16 {
    let mut req = EndTxnRequest::default();
    req.transactional_id = txn_id(id);
    req.producer_id = ProducerId(t.producer);
    req.producer_epoch = t.epoch;
    req.committed = commit;
    let Response::EndTxn(r) = rig.ask_as(who, version, Request::EndTxn(req)).await else {
        panic!("not EndTxn")
    };
    r.error_code
}

pub async fn describe_txns(rig: &Rig, who: &Caller, ids: &[&str]) -> Vec<TransactionState> {
    let mut req = DescribeTransactionsRequest::default();
    req.transactional_ids = ids.iter().map(|i| txn_id(i)).collect();
    match rig.ask_as(who, 0, Request::DescribeTransactions(req)).await {
        Response::DescribeTransactions(r) => r.transaction_states,
        other => panic!("not DescribeTransactions: {other:?}"),
    }
}

/// The transactional ids ListTransactions shows.
pub async fn list_txns(rig: &Rig, who: &Caller) -> Vec<String> {
    match rig
        .ask_as(
            who,
            1,
            Request::ListTransactions(ListTransactionsRequest::default()),
        )
        .await
    {
        Response::ListTransactions(r) => {
            assert_eq!(r.error_code, 0);
            r.transaction_states
                .iter()
                .map(|s| s.transactional_id.to_string())
                .collect()
        }
        other => panic!("not ListTransactions: {other:?}"),
    }
}
