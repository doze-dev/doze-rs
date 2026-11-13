//! Shared by the live tests of the Log and Producers pages: a real broker with a compacted topic, a topic that deletes, an empty
//! one and a transaction held open, and the Produce and transaction requests a test uses to make them.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]


use bytes::Bytes;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::batch::build::{BatchBuilder, Rec};
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::{
    EndTxnRequest, InitProducerIdRequest, ProduceRequest, ProducerId, TopicName, TransactionalId,
};

use crate::support_write::{World, world};

/// A batch of keyed records; a value of `None` is a deletion.
pub fn keyed(pairs: &[(&str, Option<&str>)]) -> Bytes {
    let recs: Vec<Rec> = pairs
        .iter()
        .map(|(k, v)| Rec {
            key: Some(k.as_bytes().to_vec()),
            value: v.map(|v| v.as_bytes().to_vec()),
            ..Rec::default()
        })
        .collect();
    BatchBuilder::from_records(1_000, &recs).build()
}

/// A batch of `n` plain records.
pub fn records(n: u32) -> Bytes {
    BatchBuilder::new().records(n).build()
}

/// A batch from a producer (idempotent, or in a transaction), first sequence `seq`.
pub fn from(producer: i64, epoch: i16, seq: i32, transactional: bool, n: u32) -> Bytes {
    BatchBuilder::new()
        .records(n)
        .attributes(if transactional { 0x10 } else { 0 })
        .timestamps(1_000, 1_000)
        .producer(producer, epoch, seq)
        .build()
}

impl World {
    /// A Produce (v9, acks=all) of one batch to one partition, as a client sends it: the partition's error code and base offset.
    pub async fn produce_batch(
        &self,
        topic: &str,
        partition: i32,
        batch: Bytes,
        txn: Option<&str>,
    ) -> (i16, i64) {
        let mut p = PartitionProduceData::default();
        p.index = partition;
        p.records = Some(batch);
        let mut t = TopicProduceData::default();
        t.name = TopicName(StrBytes::from_string(topic.to_owned()));
        t.partition_data.push(p);
        let mut r = ProduceRequest::default();
        r.acks = -1;
        r.timeout_ms = 5000;
        r.topic_data.push(t);
        r.transactional_id = txn.map(|t| TransactionalId(StrBytes::from_string(t.to_owned())));
        let Response::Produce(r) = self.ask(9, Request::Produce(r)).await else {
            panic!()
        };
        let p = &r.responses[0].partition_responses[0];
        (p.error_code, p.base_offset)
    }

    /// `n` appends of one keyed record each over `keys` keys.
    pub async fn fill(&self, topic: &str, n: usize, keys: usize) {
        for i in 0..n {
            let b = keyed(&[(&format!("k{}", i % keys), Some(&format!("v{i}")))]);
            assert_eq!(self.produce_batch(topic, 0, b, None).await.0, 0);
        }
    }

    /// InitProducerId: a producer, for a transactional id if given.
    pub async fn init_producer(&self, txn: Option<&str>) -> (i64, i16) {
        let mut r = InitProducerIdRequest::default();
        r.transactional_id = txn.map(|t| TransactionalId(StrBytes::from_string(t.to_owned())));
        r.transaction_timeout_ms = if txn.is_some() { 60_000 } else { -1 };
        let Response::InitProducerId(r) = self.ask(4, Request::InitProducerId(r)).await else {
            panic!()
        };
        assert_eq!(r.error_code, 0);
        (r.producer_id.0, r.producer_epoch)
    }

    /// AddPartitionsToTxn (v3) for one partition.
    pub async fn add_to_txn(&self, id: &str, producer: (i64, i16), topic: &str, partition: i32) {
        use kafka_wire::messages::AddPartitionsToTxnRequest;
        use kafka_wire::messages::add_partitions_to_txn_request::AddPartitionsToTxnTopic;
        let mut t = AddPartitionsToTxnTopic::default();
        t.name = TopicName(StrBytes::from_string(topic.to_owned()));
        t.partitions = vec![partition];
        let mut r = AddPartitionsToTxnRequest::default();
        r.v3_and_below_transactional_id = TransactionalId(StrBytes::from_string(id.to_owned()));
        r.v3_and_below_producer_id = ProducerId(producer.0);
        r.v3_and_below_producer_epoch = producer.1;
        r.v3_and_below_topics.push(t);
        let Response::AddPartitionsToTxn(r) = self.ask(3, Request::AddPartitionsToTxn(r)).await
        else {
            panic!()
        };
        assert_eq!(
            r.results_by_topic_v3_and_below[0].results_by_partition[0].partition_error_code,
            0
        );
    }

    /// EndTxn.
    pub async fn end_txn(&self, id: &str, producer: (i64, i16), commit: bool) -> i16 {
        let mut r = EndTxnRequest::default();
        r.transactional_id = TransactionalId(StrBytes::from_string(id.to_owned()));
        r.producer_id = ProducerId(producer.0);
        r.producer_epoch = producer.1;
        r.committed = commit;
        let Response::EndTxn(r) = self.ask(3, Request::EndTxn(r)).await else {
            panic!()
        };
        r.error_code
    }
}

/// A broker with a topic that compacts (small segments, cleaned at a low ratio), one that deletes, one that is empty and a
/// transaction held open on `orders`.
pub async fn seeded(name: &str) -> World {
    let w = world(name, |_, _| {}).await;
    w.create_topic(
        "inventory.levels",
        1,
        &[
            ("cleanup.policy", "compact"),
            ("segment.bytes", "1024"),
            ("min.cleanable.dirty.ratio", "0.01"),
            ("delete.retention.ms", "86400000"),
        ],
    )
    .await;
    w.create_topic("orders", 3, &[("segment.bytes", "1024")])
        .await;
    w.create_topic("orders.dlq", 1, &[]).await;
    w
}
