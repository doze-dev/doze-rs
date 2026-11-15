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

/// A broker on a fresh disk whose store holds `rec` already: a transactional id at an epoch no test could reach by asking
/// (one at the end of its producer id's epochs).
pub fn rig_holding(rec: &kafka_meta::TxnRecord) -> Rig {
    use kafka_storage::fs::Fs;
    let fs = kafka_storage::sim::SimFs::new();
    fs.create_dir(std::path::Path::new("/")).ok();
    let data = std::path::Path::new(super::DATA);
    kafka_storage::fs::create_dirs_durably(&fs, data).unwrap();
    let store = kafka_meta::MetaStore::open(&fs, data).unwrap();
    store.put_txn(rec).unwrap();
    drop(store);
    super::open_on(fs)
}

/// A transactional id `id` owned by producer `producer` at `epoch`, with nothing open: what [`rig_holding`] is given.
pub fn owned_at(id: &str, producer: i64, epoch: i16, timeout_ms: i32) -> kafka_meta::TxnRecord {
    let mut rec = kafka_meta::TxnRecord::new(id);
    rec.producer_id = producer;
    rec.epoch = epoch;
    rec.timeout_ms = timeout_ms;
    rec.state = kafka_meta::TxnState::CompleteCommit;
    rec.updated_ms = now_ms();
    rec
}

fn now_ms() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

impl Rig {
    /// The producers DescribeProducers lists on a partition: (producer id, epoch, start of the open transaction), sorted.
    pub async fn producers_on(&self, topic: &str, partition: i32) -> Vec<(i64, i16, i64)> {
        use kafka_wire::messages::DescribeProducersRequest;
        use kafka_wire::messages::describe_producers_request::TopicRequest;
        let mut req = DescribeProducersRequest::default();
        let mut t = TopicRequest::default();
        t.name = wire_name(topic);
        t.partition_indexes = vec![partition];
        req.topics.push(t);
        let Response::DescribeProducers(r) = self.ask(0, Request::DescribeProducers(req)).await
        else {
            panic!("not DescribeProducers")
        };
        let p = &r.topics[0].partitions[0];
        assert_eq!(p.error_code, 0);
        let mut out: Vec<_> = p
            .active_producers
            .iter()
            .map(|a| {
                let epoch = i16::try_from(a.producer_epoch).unwrap();
                (a.producer_id.0, epoch, a.current_txn_start_offset)
            })
            .collect();
        out.sort_unstable();
        out
    }

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

    /// InitProducerId v4 by a producer that says the producer id and epoch it holds (KIP-360).
    pub async fn init_txn_holding(&self, id: &str, held: Txn) -> Result<Txn, i16> {
        let mut req = InitProducerIdRequest::default();
        req.transactional_id = Some(TransactionalId(text(id)));
        req.transaction_timeout_ms = 60_000;
        req.producer_id = ProducerId(held.producer);
        req.producer_epoch = held.epoch;
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
        self.end_txn_at(3, id, t, commit).await.0
    }

    /// EndTxn at `version`: the code, and the producer id and epoch the answer carries (from v5; -1 and -1 before).
    pub async fn end_txn_at(&self, version: i16, id: &str, t: Txn, commit: bool) -> (i16, Txn) {
        let mut req = EndTxnRequest::default();
        req.transactional_id = TransactionalId(text(id));
        req.producer_id = ProducerId(t.producer);
        req.producer_epoch = t.epoch;
        req.committed = commit;
        let Response::EndTxn(r) = self.ask(version, Request::EndTxn(req)).await else {
            panic!("not EndTxn")
        };
        (
            r.error_code,
            Txn {
                producer: r.producer_id.0,
                epoch: r.producer_epoch,
            },
        )
    }

    /// EndTxn v5 that must succeed: the producer id and epoch to go on with.
    pub async fn end_v5(&self, id: &str, t: Txn, commit: bool) -> Txn {
        let (code, next) = self.end_txn_at(5, id, t, commit).await;
        assert_eq!(code, 0, "EndTxn v5 of {id} at {t:?}, commit {commit}");
        next
    }

    /// DescribeTransactions of one id: its state, producer id and epoch, and its partitions (sorted).
    pub async fn describe_txn(&self, id: &str) -> (String, Txn, Vec<(String, i32)>) {
        use kafka_wire::messages::DescribeTransactionsRequest;
        let mut req = DescribeTransactionsRequest::default();
        req.transactional_ids = vec![TransactionalId(text(id))];
        let Response::DescribeTransactions(r) =
            self.ask(0, Request::DescribeTransactions(req)).await
        else {
            panic!("not DescribeTransactions")
        };
        let s = &r.transaction_states[0];
        assert_eq!(s.error_code, 0, "DescribeTransactions of {id}");
        let mut parts: Vec<(String, i32)> = s
            .topics
            .iter()
            .flat_map(|t| t.partitions.iter().map(|p| (t.topic.0.to_string(), *p)))
            .collect();
        parts.sort();
        (
            s.transaction_state.to_string(),
            Txn {
                producer: s.producer_id.0,
                epoch: s.producer_epoch,
            },
            parts,
        )
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
        self.produce_txn_at((9, id), t, at, seq, n).await
    }

    /// A transactional Produce at `version` for transactional id `id` (from v12 it adds its partition to the transaction):
    /// the partition's answer.
    pub async fn produce_txn_at(
        &self,
        (version, id): (i16, &str),
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
        let Response::Produce(r) = self.ask(version, Request::Produce(req)).await else {
            panic!("not Produce")
        };
        r.responses[0].partition_responses[0].clone()
    }

    /// TxnOffsetCommit at `version` of one offset, as a committer that names no member: the partition's code.
    pub async fn txn_offset_at(
        &self,
        version: i16,
        id: &str,
        t: Txn,
        group: &str,
        at: (&str, i32, i64),
    ) -> i16 {
        use kafka_wire::messages::txn_offset_commit_request::{
            TxnOffsetCommitRequestPartition, TxnOffsetCommitRequestTopic,
        };
        let (topic, partition, offset) = at;
        let mut req = kafka_wire::messages::TxnOffsetCommitRequest::default();
        req.transactional_id = TransactionalId(text(id));
        req.group_id = GroupId(text(group));
        req.producer_id = ProducerId(t.producer);
        req.producer_epoch = t.epoch;
        req.generation_id = -1;
        let mut p = TxnOffsetCommitRequestPartition::default();
        p.partition_index = partition;
        p.committed_offset = offset;
        p.committed_leader_epoch = -1;
        let mut topic_req = TxnOffsetCommitRequestTopic::default();
        topic_req.name = wire_name(topic);
        topic_req.partitions.push(p);
        req.topics.push(topic_req);
        let Response::TxnOffsetCommit(r) = self.ask(version, Request::TxnOffsetCommit(req)).await
        else {
            panic!("not TxnOffsetCommit")
        };
        r.topics[0].partitions[0].error_code
    }

    /// AddPartitionsToTxn v4 or v5 (a broker's): each transaction (id, producer, verify only, topic, partitions); the
    /// top-level code and, per transaction in the order asked, the code of each partition in the order asked.
    pub async fn add_partitions_batched(
        &self,
        version: i16,
        txns: &[(&str, Txn, bool, &str, &[i32])],
    ) -> (i16, Vec<Vec<i16>>) {
        self.add_partitions_batched_as(&super::caller(), version, txns)
            .await
    }

    /// [`Rig::add_partitions_batched`] for `caller`.
    pub async fn add_partitions_batched_as(
        &self,
        caller: &kafka_broker::Caller,
        version: i16,
        txns: &[(&str, Txn, bool, &str, &[i32])],
    ) -> (i16, Vec<Vec<i16>>) {
        use kafka_wire::messages::add_partitions_to_txn_request::AddPartitionsToTxnTransaction;
        let mut req = AddPartitionsToTxnRequest::default();
        for (id, t, verify, topic, partitions) in txns {
            let mut one = AddPartitionsToTxnTransaction::default();
            one.transactional_id = TransactionalId(text(id));
            one.producer_id = ProducerId(t.producer);
            one.producer_epoch = t.epoch;
            one.verify_only = *verify;
            let mut topic_req = AddPartitionsToTxnTopic::default();
            topic_req.name = wire_name(topic);
            topic_req.partitions = partitions.to_vec();
            one.topics.push(topic_req);
            req.transactions.push(one);
        }
        let Response::AddPartitionsToTxn(r) = self
            .ask_as(caller, version, Request::AddPartitionsToTxn(req))
            .await
        else {
            panic!("not AddPartitionsToTxn")
        };
        let codes = r
            .results_by_transaction
            .iter()
            .map(|t| {
                t.topic_results
                    .iter()
                    .flat_map(|t| {
                        t.results_by_partition
                            .iter()
                            .map(|p| p.partition_error_code)
                    })
                    .collect()
            })
            .collect();
        (r.error_code, codes)
    }

    /// A read-committed Fetch (v12) of one partition from `offset`.
    pub async fn fetch_committed(&self, topic: &str, partition: i32, offset: i64) -> PartitionData {
        let mut req = super::fetch_request(&[(topic, partition, offset, 1 << 20)], 0, 1, 1 << 20);
        req.isolation_level = 1;
        self.fetch(req).await.remove(0)
    }
}
