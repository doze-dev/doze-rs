//! Our batch parser against an independent writer: batches encoded by the `kafka-protocol` crate must parse
//! here, with the fields it was told to write.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use bytes::{Bytes, BytesMut};
use indexmap::IndexMap;
use kafka_protocol::records::{
    Compression, Record, RecordBatchEncoder, RecordEncodeOptions, TimestampType,
};
use kafka_wire::batch::{Batch, Compression as Codec, TimestampType as Clock};

fn records(n: i64, producer: i64, epoch: i16, first_sequence: i32, ts: i64) -> Vec<Record> {
    (0..n)
        .map(|i| Record {
            transactional: false,
            control: false,
            delete_horizon: false,
            partition_leader_epoch: 3,
            producer_id: producer,
            producer_epoch: epoch,
            timestamp_type: TimestampType::Creation,
            offset: i,
            sequence: first_sequence + i as i32,
            timestamp: ts + i,
            key: (i % 2 == 0).then(|| Bytes::from(format!("key-{i}"))),
            value: Some(Bytes::from(format!(
                "value-{i}-{}",
                "x".repeat(i as usize * 7)
            ))),
            headers: IndexMap::new(),
        })
        .collect()
}

fn encode(recs: &[Record], compression: Compression) -> Bytes {
    let mut buf = BytesMut::new();
    let options = RecordEncodeOptions {
        version: 2,
        compression,
    };
    RecordBatchEncoder::encode(&mut buf, recs.iter(), &options).expect("the crate encodes a batch");
    buf.freeze()
}

#[test]
fn an_uncompressed_batch_parses_with_its_fields() {
    let b = Batch::parse(encode(
        &records(4, 77, 5, 100, 1_700_000_000_000),
        Compression::None,
    ))
    .unwrap();
    assert_eq!(b.record_count(), 4);
    let producer = b.producer().unwrap();
    assert_eq!(producer.id.get(), 77);
    assert_eq!(
        (producer.epoch.get(), producer.base_sequence),
        (5, Some(100))
    );
    assert_eq!(b.partition_leader_epoch(), Some(3));
    assert_eq!(b.first_timestamp().get(), 1_700_000_000_000);
    assert_eq!(b.max_timestamp().get(), 1_700_000_000_003);
    assert_eq!(b.attributes().compression(), Codec::None);
    assert_eq!(b.attributes().timestamp_type(), Clock::Create);
    assert!(!b.attributes().is_transactional() && !b.attributes().is_control());
}

#[test]
fn a_non_idempotent_batch_has_no_producer() {
    let b = Batch::parse(encode(&records(1, -1, -1, -1, 5), Compression::None)).unwrap();
    assert!(b.producer().is_none());
}

#[test]
fn transactional_and_control_bits_are_read() {
    let mut recs = records(2, 9, 0, 0, 0);
    for r in &mut recs {
        r.transactional = true;
    }
    let b = Batch::parse(encode(&recs, Compression::None)).unwrap();
    assert!(b.attributes().is_transactional());
}

#[test]
fn stamping_survives_the_crates_view() {
    // What we stamp, the crate's decoder still reads as a valid batch with the same records.
    let original = encode(&records(3, 1, 0, 0, 10), Compression::None);
    let batch = Batch::parse(original).unwrap();
    let mut bytes = batch
        .stamped(kafka_wire::ids::Offset::new(500).unwrap(), None)
        .to_bytes();
    let decoded = kafka_protocol::records::RecordBatchDecoder::decode(&mut bytes).unwrap();
    assert_eq!(
        decoded.records.iter().map(|r| r.offset).collect::<Vec<_>>(),
        vec![500, 501, 502]
    );
}
