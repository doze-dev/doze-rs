use super::*;
use crate::batch::build::{BatchBuilder, Rec, encode_records};
use std::io::Write;

/// Five records at offsets 100, 102, 103, 107, 109 (gaps, as a batch compaction already thinned, or a producer, has), with
/// keys a, b, a, c, c and the last a deletion; timestamps 1000 + 10 * place.
fn recs() -> Vec<Rec> {
    [(0, "a"), (2, "b"), (3, "a"), (7, "c"), (9, "c")]
        .into_iter()
        .enumerate()
        .map(|(i, (delta, key))| Rec {
            timestamp_delta: i as i64 * 10,
            offset_delta: Some(delta),
            key: Some(key.as_bytes().to_vec()),
            value: (i != 4).then(|| vec![b'v'; 50]),
            headers: vec![],
        })
        .collect()
}

fn batch() -> Batch {
    Batch::parse_stored(
        BatchBuilder::new()
            .base_offset(100)
            .records(5)
            .last_offset_delta(9)
            .timestamps(1_000, 1_040)
            .producer(7, 2, 40)
            .body(encode_records(&recs()))
            .build(),
    )
    .unwrap()
}

const RULES: Rules = Rules {
    max_inflated: 1 << 20,
    horizon: 50_000,
    marker_of_empty_transaction: false,
};

fn offsets(b: &Batch) -> Vec<(i64, i64)> {
    b.record_times()
        .unwrap()
        .map(|r| r.unwrap())
        .map(|r| (100 + i64::from(r.offset_delta), r.timestamp.get()))
        .collect()
}

#[test]
fn keeping_every_record_changes_nothing_and_hands_back_the_same_bytes() {
    let same = Batch::parse_stored(
        BatchBuilder::new()
            .base_offset(100)
            .records(2)
            .last_offset_delta(9)
            .body(encode_records(&recs()[..2]))
            .build(),
    )
    .unwrap();
    let f = same.filter(RULES, |_| true).unwrap();
    assert!(!f.changed);
    assert_eq!(f.batch.as_bytes(), same.as_bytes());
    // Taking out only the deletion changes the batch, and gives it no horizon (it keeps no deletion).
    let g = batch().filter(RULES, |s| !s.tombstone).unwrap();
    assert_eq!(
        (g.kept, g.changed, g.batch.delete_horizon()),
        (4, true, None)
    );
}

#[test]
fn the_records_kept_keep_their_offsets_and_times_and_the_batch_its_place_span_and_producer() {
    let f = batch()
        .filter(RULES, |s| s.key == Some(b"a") || s.key == Some(b"b"))
        .unwrap();
    assert!(f.changed);
    let b = &f.batch;
    assert_eq!(f.kept, 3);
    assert_eq!(offsets(b), [(100, 1_000), (102, 1_010), (103, 1_020)]);
    assert_eq!(
        (b.base_offset(), b.next_offset()),
        (Offset::new(100), Offset::new(110))
    );
    let p = b.producer().unwrap();
    assert_eq!(
        (p.id.get(), p.epoch.get(), p.base_sequence),
        (7, 2, Some(40))
    );
    assert_eq!(b.max_timestamp().get(), 1_020, "the newest of what is left");
    assert!(
        Batch::parse_stored(b.as_bytes().clone()).is_ok(),
        "length and checksum are right"
    );
}

#[test]
fn a_batch_with_nothing_kept_is_empty_but_still_spans_its_offsets_and_names_its_producer() {
    let f = batch().filter(RULES, |_| false).unwrap();
    let b = &f.batch;
    assert_eq!((f.kept, b.record_count()), (0, 0));
    assert_eq!(b.next_offset(), Offset::new(110));
    assert_eq!(b.producer().unwrap().base_sequence, Some(40));
    assert_eq!(b.max_timestamp().get(), 1_040, "as it was");
    assert_eq!(b.attributes().compression(), Compression::None);
    assert!(b.delete_horizon().is_none());
}

#[test]
fn a_deletion_kept_gives_the_batch_a_horizon_and_every_record_keeps_its_time() {
    let f = batch().filter(RULES, |_| true).unwrap();
    let b = &f.batch;
    assert_eq!(b.delete_horizon(), Some(50_000));
    assert_eq!(b.first_timestamp().get(), 50_000);
    let want: Vec<_> = [
        (100, 1_000),
        (102, 1_010),
        (103, 1_020),
        (107, 1_030),
        (109, 1_040),
    ]
    .to_vec();
    assert_eq!(offsets(b), want);
    // Done again, it keeps the horizon it has and changes nothing more.
    let again = b
        .filter(
            Rules {
                horizon: 99_999,
                ..RULES
            },
            |_| true,
        )
        .unwrap();
    assert!(!again.changed);
    assert_eq!(again.batch.delete_horizon(), Some(50_000));
    // And what it is told of each record is the record: offset, key, whether it is a deletion.
    let mut seen = Vec::new();
    batch()
        .filter(RULES, |s| {
            seen.push((s.offset, s.key.map(<[u8]>::to_vec), s.tombstone));
            true
        })
        .unwrap();
    assert_eq!(seen[4], (109, Some(b"c".to_vec()), true));
    assert_eq!(seen[1], (102, Some(b"b".to_vec()), false));
}

#[test]
fn a_marker_of_a_transaction_with_nothing_left_is_given_a_horizon_and_other_markers_are_not() {
    let marker = Batch::parse_stored(
        BatchBuilder::new()
            .base_offset(5)
            .attributes(0x30)
            .producer(7, 2, -1)
            .timestamps(77, 77)
            .build(),
    )
    .unwrap();
    let plain = marker.filter(RULES, |_| true).unwrap();
    assert!(!plain.changed);
    let f = marker
        .filter(
            Rules {
                marker_of_empty_transaction: true,
                ..RULES
            },
            |_| true,
        )
        .unwrap();
    assert_eq!(f.batch.delete_horizon(), Some(50_000));
    assert!(f.batch.attributes().is_control());
}

#[test]
fn a_compressed_batch_stays_in_its_codec_and_an_emptied_one_is_not_compressed() {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(&encode_records(&recs())).unwrap();
    let packed = e.finish().unwrap();
    let b = Batch::parse_stored(
        BatchBuilder::new()
            .base_offset(100)
            .records(5)
            .last_offset_delta(9)
            .attributes(1)
            .timestamps(1_000, 1_040)
            .body(packed)
            .build(),
    )
    .unwrap();
    let f = b.filter(RULES, |s| s.key == Some(b"a")).unwrap();
    assert_eq!(
        (f.kept, f.batch.attributes().compression()),
        (2, Compression::Gzip)
    );
    let mut inflated = Vec::new();
    inflate::open(
        Compression::Gzip,
        &f.batch.as_bytes()[HEADER_LEN..],
        1 << 20,
    )
    .unwrap()
    .read_to_end(&mut inflated)
    .unwrap();
    assert_eq!(
        inflated,
        encode_records(&[recs()[0].clone(), recs()[2].clone()])
    );
    let none = b.filter(RULES, |_| false).unwrap();
    assert_eq!(none.batch.attributes().compression(), Compression::None);
}
