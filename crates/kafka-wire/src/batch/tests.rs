use super::build::{BatchBuilder, reseal};
use super::*;

fn ok() -> Vec<u8> {
    BatchBuilder::new()
        .records(3)
        .producer(7, 2, 40)
        .timestamps(100, 130)
        .build()
        .to_vec()
}

fn parse(v: &[u8]) -> Result<Batch, BatchError> {
    Batch::parse(Bytes::copy_from_slice(v))
}

#[test]
fn a_valid_batch_exposes_its_header() {
    let b = parse(&ok()).unwrap();
    assert_eq!(b.record_count(), 3);
    assert_eq!(b.base_offset(), Some(Offset::ZERO));
    let p = b.producer().unwrap();
    assert_eq!(
        (p.id.get(), p.epoch.get(), p.base_sequence),
        (7, 2, Some(40))
    );
    assert_eq!(
        (b.first_timestamp().get(), b.max_timestamp().get()),
        (100, 130)
    );
    assert_eq!(b.attributes().compression(), Compression::None);
    assert_eq!(b.partition_leader_epoch(), Some(0));
    assert!(
        parse(&BatchBuilder::new().build())
            .unwrap()
            .producer()
            .is_none()
    );
}

#[test]
fn shape_rules() {
    assert_eq!(parse(&ok()[..30]).unwrap_err(), BatchError::TooShort(30));

    let mut v = ok();
    v[16] = 1; // magic 1 is the old message format
    assert_eq!(parse(&v).unwrap_err(), BatchError::Magic(1));

    let mut v = ok();
    v.push(0); // a byte the header does not account for
    assert!(matches!(parse(&v).unwrap_err(), BatchError::Length { .. }));

    // The count and lastOffsetDelta must agree (and the CRC is fixed so only that is wrong).
    let mut v = ok();
    v[23..27].copy_from_slice(&9i32.to_be_bytes());
    reseal(&mut v);
    assert_eq!(
        parse(&v).unwrap_err(),
        BatchError::Count {
            count: 3,
            last_offset_delta: 9
        }
    );

    // A count of zero or less is never a batch.
    let v = BatchBuilder::new().records(0).body(vec![]).build();
    assert!(matches!(
        Batch::parse(v).unwrap_err(),
        BatchError::Count { .. }
    ));
}

#[test]
fn checks_run_in_kafkas_order_so_the_first_fault_is_the_one_reported() {
    // A bad count and a bad CRC: the checksum is looked at first.
    let mut v = BatchBuilder::new().records(3).build().to_vec();
    v[23..27].copy_from_slice(&9i32.to_be_bytes());
    assert!(matches!(parse(&v).unwrap_err(), BatchError::Crc { .. }));
    // A codec number Kafka does not have, with a good CRC and a bad count: the codec is reported.
    let mut v = BatchBuilder::new()
        .records(3)
        .attributes(5)
        .build()
        .to_vec();
    v[23..27].copy_from_slice(&9i32.to_be_bytes());
    reseal(&mut v);
    assert_eq!(parse(&v).unwrap_err(), BatchError::UnknownCodec(5));
}

#[test]
fn refusals_are_answered_as_kafka_answers_them() {
    let unknown = BatchError::UnknownCodec(6);
    let bad_count = BatchError::Count {
        count: 0,
        last_offset_delta: 0,
    };
    assert_eq!(BatchError::TooShort(1).refusal(), Refusal::CorruptMessage);
    assert_eq!(BatchError::Magic(1).refusal(), Refusal::CorruptMessage);
    assert_eq!(unknown.refusal(), Refusal::UnknownServerError);
    assert_eq!(bad_count.refusal(), Refusal::InvalidRecord);
    assert_eq!(
        BatchError::TooManyRecords(1).refusal(),
        Refusal::InvalidRecord
    );
    let located = BatchError::InPayload {
        at: 9,
        error: Box::new(bad_count),
    };
    assert_eq!(located.refusal(), Refusal::InvalidRecord);
}

fn with_count(records: u32, attributes: u16, body: usize) -> Result<Batch, BatchError> {
    Batch::parse(
        BatchBuilder::new()
            .records(records)
            .attributes(attributes)
            .body(vec![0; body])
            .build(),
    )
}

#[test]
fn the_count_limits_are_exact() {
    // Uncompressed: a record takes at least a byte, so the count may equal the body and no more.
    assert!(with_count(8, 0, 8).is_ok());
    assert!(matches!(
        with_count(9, 0, 8).unwrap_err(),
        BatchError::CountExceedsBody { count: 9, body: 8 }
    ));
    // A 69-byte batch claiming 2 billion records would move the next offset to 2147483647.
    assert!(matches!(
        with_count(2_000_000_000, 0, 8).unwrap_err(),
        BatchError::CountExceedsBody { .. }
    ));
    // Compressed: dense, but only so dense (16 Mi records).
    assert!(with_count(16 << 20, 1, 8).is_ok());
    assert_eq!(
        with_count((16 << 20) + 1, 1, 8).unwrap_err(),
        BatchError::TooManyRecords((16 << 20) + 1)
    );
}

#[test]
fn a_wrong_crc_is_refused() {
    let mut v = ok();
    let last = v.len() - 1;
    v[last] ^= 0xff;
    assert!(matches!(parse(&v).unwrap_err(), BatchError::Crc { .. }));
    // The base offset is not covered, so changing it is not a CRC error.
    let mut v = ok();
    v[0] = 9;
    assert!(parse(&v).is_ok());
}

/// The byte positions where two batches of the same size differ.
fn differing(a: &Bytes, b: &Bytes) -> Vec<usize> {
    assert_eq!(a.len(), b.len());
    (0..a.len()).filter(|&i| a[i] != b[i]).collect()
}

#[test]
fn stamping_the_base_offset_changes_only_the_base_offset() {
    let before = parse(&ok()).unwrap();
    let after = before.clone().with_base_offset(Offset::new(1234).unwrap());
    assert_eq!(after.base_offset(), Offset::new(1234));
    assert!(Batch::parse(after.as_bytes().clone()).is_ok());
    assert!(
        differing(before.as_bytes(), after.as_bytes())
            .iter()
            .all(|&i| i < 8)
    );
}

#[test]
fn log_append_time_changes_only_the_clock_bit_the_max_timestamp_and_the_crc() {
    let before = parse(&ok()).unwrap();
    let after = before
        .clone()
        .with_log_append_time(TimestampMs::from_wire(9_000));
    assert_eq!(
        after.attributes().timestamp_type(),
        TimestampType::LogAppend
    );
    assert_eq!(after.max_timestamp().get(), 9_000);
    assert_eq!(after.first_timestamp(), before.first_timestamp());
    assert_eq!(after.producer(), before.producer());
    assert!(
        Batch::parse(after.as_bytes().clone()).is_ok(),
        "the CRC was recomputed"
    );
    // Byte for byte: the CRC (17..21), the attributes (21..23) and the max timestamp (35..43).
    let changed = differing(before.as_bytes(), after.as_bytes());
    assert!(
        changed
            .iter()
            .all(|i| (17..23).contains(i) || (35..43).contains(i)),
        "bytes changed at {changed:?}"
    );
}

#[test]
fn a_run_of_batches_is_walked_one_at_a_time() {
    let one = BatchBuilder::new().records(2).build();
    let two = BatchBuilder::new().records(5).build();
    let mut run = one.to_vec();
    run.extend_from_slice(&two);
    let counts: Vec<_> = Batches::new(Bytes::from(run.clone()))
        .map(|b| b.unwrap().record_count())
        .collect();
    assert_eq!(counts, vec![2, 5]);
    assert_eq!(Batches::new(Bytes::new()).count(), 0);

    // A batch cut short, and bytes after the last one, are reported once and end the walk.
    run.pop();
    let mut walk = Batches::new(Bytes::from(run));
    assert!(walk.next().unwrap().is_ok());
    assert_eq!(
        walk.next().unwrap().unwrap_err(),
        BatchError::Truncated(one.len())
    );
    assert!(walk.next().is_none());
    let mut trailing = one.to_vec();
    trailing.extend_from_slice(&[1, 2, 3]);
    assert!(Batches::new(Bytes::from(trailing)).any(|b| b.is_err()));

    // A bad batch says where it starts.
    let mut bad = one.to_vec();
    let mut second = two.to_vec();
    let last = second.len() - 1;
    second[last] ^= 1;
    bad.extend_from_slice(&second);
    let Some(Err(BatchError::InPayload { at, .. })) = Batches::new(Bytes::from(bad)).nth(1) else {
        panic!("expected a located error");
    };
    assert_eq!(at, one.len());
}

#[test]
fn attribute_bits() {
    let a = Attributes::from_bits(0b11_0100);
    assert_eq!(a.compression(), Compression::Zstd);
    assert!(a.is_transactional() && a.is_control());
    assert_eq!(a.timestamp_type(), TimestampType::Create);
    assert_eq!(
        Attributes::from_bits(0x08).timestamp_type(),
        TimestampType::LogAppend
    );
    assert_eq!(
        Attributes::from_bits(7).compression(),
        Compression::Unknown(7)
    );
    assert!(!Attributes::from_bits(0).is_control());
}
