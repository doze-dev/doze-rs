//! Requests for the transaction APIs.
#![allow(
    unreachable_pub,
    dead_code,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::{Rig, produce_request, wire_name};
use bytes::Bytes;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::add_partitions_to_txn_request::AddPartitionsToTxnTopic;
use kafka_wire::messages::fetch_response::PartitionData;
use kafka_wire::messages::{
    AddOffsetsToTxnRequest, AddPartitionsToTxnRequest, EndTxnRequest, GroupId,
    InitProducerIdRequest, ProducerId, TransactionalId,
};

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

/// A producer that owns a transactional id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Txn {
    pub producer: i64,
    pub epoch: i16,
}

impl Rig {
    /// InitProducerId for a transactional id (timeout in ms): the producer it is to use, or the code.
    pub async fn init_txn(&self, id: &str, timeout_ms: i32) -> Result<Txn, i16> {
        let mut req = InitProducerIdRequest::default();
        req.transactional_id = Some(TransactionalId(text(id)));
        req.transaction_timeout_ms = timeout_ms;
        let Response::InitProducerId(r) = self.ask(4, Request::InitProducerId(req)).await else {
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

    /// AddPartitionsToTxn (v3): the code of each partition of `topic`.
    pub async fn add_partitions(
        &self,
        id: &str,
        t: Txn,
        topic: &str,
        partitions: &[i32],
    ) -> Vec<i16> {
        let mut req = AddPartitionsToTxnRequest::default();
        req.v3_and_below_transactional_id = TransactionalId(text(id));
        req.v3_and_below_producer_id = ProducerId(t.producer);
        req.v3_and_below_producer_epoch = t.epoch;
        let mut topic_req = AddPartitionsToTxnTopic::default();
        topic_req.name = wire_name(topic);
        topic_req.partitions = partitions.to_vec();
        req.v3_and_below_topics.push(topic_req);
        let Response::AddPartitionsToTxn(r) = self.ask(3, Request::AddPartitionsToTxn(req)).await
        else {
            panic!("not AddPartitionsToTxn")
        };
        r.results_by_topic_v3_and_below
            .iter()
            .flat_map(|t| {
                t.results_by_partition
                    .iter()
                    .map(|p| p.partition_error_code)
            })
            .collect()
    }

    pub async fn add_offsets(&self, id: &str, t: Txn, group: &str) -> i16 {
        let mut req = AddOffsetsToTxnRequest::default();
        req.transactional_id = TransactionalId(text(id));
        req.producer_id = ProducerId(t.producer);
        req.producer_epoch = t.epoch;
        req.group_id = GroupId(text(group));
        let Response::AddOffsetsToTxn(r) = self.ask(3, Request::AddOffsetsToTxn(req)).await else {
            panic!("not AddOffsetsToTxn")
        };
        r.error_code
    }

    /// TxnOffsetCommit (v3): the code of each partition.
    pub async fn txn_offset(
        &self,
        id: &str,
        t: Txn,
        group: &str,
        at: (&str, i32),
        offset: i64,
    ) -> i16 {
        self.txn_offset_as(id, t, group, ("", -1), (at.0, at.1, offset))
            .await
    }

    /// TxnOffsetCommit by a committer that names its member id and generation (or epoch).
    pub async fn txn_offset_as(
        &self,
        id: &str,
        t: Txn,
        group: &str,
        committer: (&str, i32),
        at: (&str, i32, i64),
    ) -> i16 {
        let (topic, partition, offset) = at;
        use kafka_wire::messages::txn_offset_commit_request::{
            TxnOffsetCommitRequestPartition, TxnOffsetCommitRequestTopic,
        };
        let mut req = kafka_wire::messages::TxnOffsetCommitRequest::default();
        req.transactional_id = TransactionalId(text(id));
        req.group_id = GroupId(text(group));
        req.producer_id = ProducerId(t.producer);
        req.producer_epoch = t.epoch;
        req.member_id = text(committer.0);
        req.generation_id = committer.1;
        let mut p = TxnOffsetCommitRequestPartition::default();
        p.partition_index = partition;
        p.committed_offset = offset;
        p.committed_leader_epoch = -1;
        let mut topic_req = TxnOffsetCommitRequestTopic::default();
        topic_req.name = wire_name(topic);
        topic_req.partitions.push(p);
        req.topics.push(topic_req);
        let Response::TxnOffsetCommit(r) = self.ask(3, Request::TxnOffsetCommit(req)).await else {
            panic!("not TxnOffsetCommit")
        };
        r.topics[0].partitions[0].error_code
    }

    pub async fn end_txn(&self, id: &str, t: Txn, commit: bool) -> i16 {
        let mut req = EndTxnRequest::default();
        req.transactional_id = TransactionalId(text(id));
        req.producer_id = ProducerId(t.producer);
        req.producer_epoch = t.epoch;
        req.committed = commit;
        let Response::EndTxn(r) = self.ask(3, Request::EndTxn(req)).await else {
            panic!("not EndTxn")
        };
        r.error_code
    }

    /// A transactional Produce (v9): the partition's answer.
    pub async fn produce_txn(
        &self,
        id: &str,
        t: Txn,
        at: (&str, i32),
        seq: i32,
        n: u32,
    ) -> kafka_wire::messages::produce_response::PartitionProduceResponse {
        let (topic, partition) = at;
        let batch: Bytes = kafka_wire::batch::build::BatchBuilder::new()
            .records(n)
            .attributes(0x10)
            .timestamps(1_000, 1_000)
            .producer(t.producer, t.epoch, seq)
            .build();
        let mut req = produce_request(-1, topic, &[(partition, batch)]);
        req.transactional_id = Some(TransactionalId(text(id)));
        let Response::Produce(r) = self.ask(9, Request::Produce(req)).await else {
            panic!("not Produce")
        };
        r.responses[0].partition_responses[0].clone()
    }

    /// A read-committed Fetch (v12) of one partition from `offset`.
    pub async fn fetch_committed(&self, topic: &str, partition: i32, offset: i64) -> PartitionData {
        let mut req = super::fetch_request(&[(topic, partition, offset, 1 << 20)], 0, 1, 1 << 20);
        req.isolation_level = 1;
        self.fetch(req).await.remove(0)
    }
}
