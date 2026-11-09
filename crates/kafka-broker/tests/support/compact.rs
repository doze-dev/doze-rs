//! Helpers for tests that run producers, transactions, DeleteRecords and cleaning against one compacted topic `c`.
#![allow(
    unreachable_pub,
    dead_code,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::{Rig, produce_request, topic, wire_name, with_config};
use bytes::Bytes;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::batch::Batches;
use kafka_wire::batch::build::{BatchBuilder, Rec};
use kafka_wire::messages::DeleteRecordsRequest;
use kafka_wire::messages::TransactionalId;
use kafka_wire::messages::create_topics_request::CreatableTopic;
use kafka_wire::messages::delete_records_request::{DeleteRecordsPartition, DeleteRecordsTopic};

/// Topic `c`, one partition, small segments and a low dirty ratio, so that a few records are cleaned.
pub fn compacted(policy: &str) -> CreatableTopic {
    let t = with_config(topic("c", 1), "cleanup.policy", policy);
    let t = with_config(t, "segment.bytes", "1024");
    with_config(t, "min.cleanable.dirty.ratio", "0.01")
}

/// A batch of records (key, value) from a producer, optionally inside a transaction.
pub fn from(
    producer: i64,
    epoch: i16,
    seq: i32,
    transactional: bool,
    pairs: &[(&str, &str)],
) -> Bytes {
    let recs: Vec<Rec> = pairs
        .iter()
        .map(|(k, v)| Rec {
            key: Some(k.as_bytes().to_vec()),
            value: Some(v.as_bytes().to_vec()),
            ..Rec::default()
        })
        .collect();
    BatchBuilder::from_records(1_000, &recs)
        .attributes(if transactional { 0x10 } else { 0 })
        .producer(producer, epoch, seq)
        .build()
}

/// A Produce (v9) of one batch to `c` partition 0 for transactional id `id`: (error, base offset).
pub async fn txn_produce(rig: &Rig, id: &str, batch: Bytes) -> (i16, i64) {
    let mut req = produce_request(-1, "c", &[(0, batch)]);
    req.transactional_id = Some(TransactionalId(StrBytes::from_string(id.to_owned())));
    let Response::Produce(r) = rig.ask(9, Request::Produce(req)).await else {
        panic!("not Produce")
    };
    let p = &r.responses[0].partition_responses[0];
    (p.error_code, p.base_offset)
}

/// DeleteRecords on `c` partition 0: (error, low watermark).
pub async fn delete_records(rig: &Rig, offset: i64) -> (i16, i64) {
    let mut req = DeleteRecordsRequest::default();
    req.timeout_ms = 5_000;
    let mut t = DeleteRecordsTopic::default();
    t.name = wire_name("c");
    let mut p = DeleteRecordsPartition::default();
    p.partition_index = 0;
    p.offset = offset;
    t.partitions.push(p);
    req.topics.push(t);
    let Response::DeleteRecords(r) = rig.ask(2, Request::DeleteRecords(req)).await else {
        panic!("not DeleteRecords")
    };
    let p = &r.topics[0].partitions[0];
    (p.error_code, p.low_watermark)
}

/// Reads `c` as a consumer does, from `from`, to the end, in either isolation level: (offset, key) of each data record.
/// A fetch that returns nothing before the end fails the test.
pub async fn consume(rig: &Rig, from: i64, committed: bool) -> Vec<(i64, String)> {
    let end = rig.list_offset(7, "c", 0, -1).await.offset;
    let (mut at, mut out) = (from, Vec::new());
    while at < end {
        let data = if committed {
            rig.fetch_committed("c", 0, at).await
        } else {
            rig.fetch_now(&[("c", 0, at, 1 << 20)]).await.remove(0)
        };
        assert_eq!(data.error_code, 0, "fetch at {at}");
        let before = at;
        for b in Batches::new(data.records.unwrap_or_default()) {
            let b = b.unwrap();
            b.each_record(1 << 20, |s| {
                if s.offset >= at && !b.attributes().is_control() {
                    out.push((
                        s.offset,
                        String::from_utf8(s.key.unwrap().to_vec()).unwrap(),
                    ));
                }
            })
            .unwrap();
            at = b.next_offset().unwrap().get();
        }
        if at == before {
            // A read_committed fetch held by an open transaction stops here; the consumer would ask again.
            assert!(committed, "a fetch at {at} returns no batch, end {end}");
            break;
        }
    }
    out
}

/// `n` rounds of the cleaner, now.
pub async fn clean_rounds(rig: &Rig, n: usize) {
    for _ in 0..n {
        rig.broker.check_cleaner().await;
    }
}
