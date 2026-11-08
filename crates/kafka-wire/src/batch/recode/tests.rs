use super::*;
use crate::batch::build::{BatchBuilder, Rec, encode_records};
use crate::batch::scan::ScanRules;
use std::io::Write;

fn recs() -> Vec<Rec> {
    (0..5)
        .map(|i| Rec {
            timestamp_delta: i * 3,
            offset_delta: Some(i * 7), // skipping, as a producer may write
            key: Some(vec![b'k'; i as usize]),
            value: Some(vec![b'v'; 300]),
            headers: vec![(b"h".to_vec(), b"x".to_vec())],
        })
        .collect()
}

fn rules(renumbering: bool) -> ScanRules {
    ScanRules {
        require_keys: false,
        log_append_time: false,
        now: TimestampMs::from_wire(1_000),
        before_max_ms: i64::MAX,
        after_max_ms: i64::MAX,
        max_inflated: 1 << 20,
        renumbering,
    }
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

#[test]
fn a_batch_is_written_again_in_each_codec_with_its_records_numbered_and_otherwise_as_they_were() {
    let original = Batch::parse(
        BatchBuilder::new()
            .records(5)
            .attributes(1)
            .timestamps(1_000, 9_999)
            .body(gzip(&encode_records(&recs())))
            .build(),
    )
    .unwrap();
    for target in [
        Compression::None,
        Compression::Gzip,
        Compression::Snappy,
        Compression::Lz4,
        Compression::Zstd,
    ] {
        let again = original
            .recode(target, 1 << 20, TimestampMs::from_wire(1_012))
            .unwrap();
        assert_eq!(again.attributes().compression(), target, "{target:?}");
        assert_eq!(again.max_timestamp().get(), 1_012);
        assert_eq!(again.record_count(), 5);
        assert_eq!(again.first_timestamp(), original.first_timestamp());
        // Read back through the scan: the records are the same (their times, and 0..5 now).
        let scanned = again
            .scan(&rules(false), &"t-0")
            .unwrap_or_else(|e| panic!("{target:?}: {}", e.reason));
        assert_eq!(scanned.max_timestamp.get(), 1_012);
    }
}

#[test]
fn the_records_survive_the_trip_byte_for_byte_but_for_their_offset_deltas() {
    let skipping = recs();
    let plain = Batch::parse(BatchBuilder::from_records(1_000, &skipping).build()).unwrap();
    let again = plain
        .recode(Compression::None, 1 << 20, TimestampMs::from_wire(1_012))
        .unwrap();
    let renumbered: Vec<Rec> = skipping
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, r)| Rec {
            offset_delta: Some(i as i64),
            ..r
        })
        .collect();
    assert_eq!(
        &again.as_bytes()[HEADER_LEN..],
        &encode_records(&renumbered)[..]
    );
}

#[test]
fn a_batch_that_inflates_past_the_limit_is_too_large_to_write_again() {
    let big = Rec {
        timestamp_delta: 0,
        offset_delta: None,
        key: None,
        value: Some(vec![0; 2 << 20]),
        headers: vec![],
    };
    let b = Batch::parse(
        BatchBuilder::new()
            .records(1)
            .attributes(1)
            .body(gzip(&encode_records(&[big])))
            .build(),
    )
    .unwrap();
    let refused = b
        .recode(Compression::None, 1 << 20, TimestampMs::from_wire(0))
        .unwrap_err();
    assert_eq!(refused.refusal, Refusal::MessageTooLarge);
}
