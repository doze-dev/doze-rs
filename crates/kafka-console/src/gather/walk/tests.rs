use bytes::{Bytes, BytesMut};
use kafka_wire::batch::build::{BatchBuilder, Rec};

use super::*;

const KEEP: Keep = Keep {
    key: 16,
    value: 8,
    header: 8,
};

fn rec(key: &str, value: &str) -> Rec {
    Rec {
        timestamp_delta: 1,
        offset_delta: None,
        key: Some(key.as_bytes().to_vec()),
        value: Some(value.as_bytes().to_vec()),
        headers: vec![("h".as_bytes().to_vec(), "v".as_bytes().to_vec())],
    }
}

fn batch(base: i64, n: usize, attributes: u16, producer: Option<i64>) -> Bytes {
    let recs: Vec<Rec> = (0..n)
        .map(|i| rec(&format!("k{}", base + i as i64), "a-long-value"))
        .collect();
    let mut b = BatchBuilder::from_records(1_000, &recs)
        .base_offset(base)
        .last_offset_delta(n as i32 - 1)
        .attributes(attributes);
    if let Some(p) = producer {
        b = b.producer(p, 2, 100);
    }
    b.build()
}

/// A transaction marker batch at `base`: control and transactional, one record whose key is version 0 and type 0 (abort).
fn marker(base: i64, producer: i64) -> Bytes {
    let recs = [Rec {
        timestamp_delta: 0,
        offset_delta: None,
        key: Some(vec![0, 0, 0, 0]),
        value: Some(vec![0; 6]),
        headers: vec![],
    }];
    BatchBuilder::from_records(1_000, &recs)
        .base_offset(base)
        .last_offset_delta(0)
        .attributes(0x30)
        .producer(producer, 2, 0)
        .build()
}

fn joined(parts: &[Bytes]) -> Bytes {
    let mut all = BytesMut::new();
    for p in parts {
        all.extend_from_slice(p);
    }
    all.freeze()
}

fn standings(w: &Walked) -> Vec<(i64, Standing)> {
    w.rows.iter().map(|r| (r.offset, r.standing)).collect()
}

#[test]
fn records_at_or_above_the_last_stable_offset_are_held_and_the_rest_are_committed() {
    let w = walk(&batch(100, 3, 0, None), 0, 100, 101, &[], (KEEP, 100));
    assert_eq!(
        standings(&w),
        vec![
            (100, Standing::Committed),
            (101, Standing::AboveLso),
            (102, Standing::AboveLso)
        ]
    );
    assert!(!w.saw_txn);
    assert_eq!(w.next, Some(103));
}

#[test]
fn keys_values_and_headers_are_kept_to_the_limit_and_counted_in_full() {
    let w = walk(&batch(0, 1, 0, None), 3, 0, 10, &[], (KEEP, 100));
    let r = &w.rows[0];
    assert_eq!((r.partition, r.offset, r.timestamp), (3, 0, 1_001));
    assert_eq!(r.key.as_deref(), Some(&b"k0"[..]));
    assert_eq!(r.value.as_deref(), Some(&b"a-long-v"[..]));
    assert_eq!((r.key_len, r.value_len), (2, 12));
    assert_eq!(r.headers, vec![(b"h".to_vec(), Some(b"v".to_vec()), 1)]);
    assert_eq!(r.codec, "none");
    assert!(!r.log_append);
}

#[test]
fn the_batch_that_holds_the_offset_may_begin_before_it() {
    let w = walk(&batch(10, 5, 0, None), 0, 12, 100, &[], (KEEP, 100));
    assert_eq!(
        w.rows.iter().map(|r| r.offset).collect::<Vec<_>>(),
        vec![12, 13, 14]
    );
}

#[test]
fn the_limit_stops_the_walk() {
    let all = joined(&[batch(0, 5, 0, None), batch(5, 5, 0, None)]);
    let w = walk(&all, 0, 0, 100, &[], (KEEP, 7));
    assert_eq!(w.rows.len(), 7);
}

#[test]
fn an_aborted_transaction_is_skipped_from_its_first_offset_to_its_marker() {
    // Producer 7 wrote offsets 10 to 12 and aborted (marker at 13); producer 8 wrote 14 and 15 and is still open.
    let all = joined(&[
        batch(8, 2, 0, None),
        batch(10, 3, 0x10, Some(7)),
        marker(13, 7),
        batch(14, 2, 0x10, Some(8)),
        batch(16, 1, 0, None),
    ]);
    let w = walk(&all, 0, 8, 17, &[(7, 10)], (KEEP, 100));
    assert!(w.saw_txn);
    assert_eq!(
        standings(&w),
        vec![
            (8, Standing::Committed),
            (9, Standing::Committed),
            (10, Standing::Aborted),
            (11, Standing::Aborted),
            (12, Standing::Aborted),
            (14, Standing::Committed),
            (15, Standing::Committed),
            (16, Standing::Committed),
        ],
        "the marker is not a row, and 8's records are not aborted"
    );
    // The same answer with the list left out: nothing is known to be aborted.
    let none = walk(&all, 0, 8, 17, &[], (KEEP, 100));
    assert!(none.rows.iter().all(|r| r.standing == Standing::Committed));
}

#[test]
fn a_producer_after_its_marker_is_not_aborted_again() {
    let all = joined(&[
        batch(10, 1, 0x10, Some(7)),
        marker(11, 7),
        batch(12, 1, 0x10, Some(7)),
    ]);
    let w = walk(&all, 0, 10, 50, &[(7, 10)], (KEEP, 100));
    assert_eq!(
        standings(&w),
        vec![(10, Standing::Aborted), (12, Standing::Committed)]
    );
}

#[test]
fn a_window_that_begins_inside_the_transaction_still_skips_it() {
    // The Fetch began at 11; the aborted list says it began at 10 (before the window).
    let all = joined(&[batch(10, 3, 0x10, Some(7)), marker(13, 7)]);
    let w = walk(&all, 0, 11, 50, &[(7, 10)], (KEEP, 100));
    assert_eq!(
        standings(&w),
        vec![(11, Standing::Aborted), (12, Standing::Aborted)]
    );
}

#[test]
fn the_producer_and_the_sequence_of_each_record_are_reported() {
    let w = walk(
        &batch(20, 3, 0x10, Some(4017)),
        0,
        20,
        100,
        &[],
        (KEEP, 100),
    );
    assert_eq!(w.rows[0].producer, Some((4017, 2, 100)));
    assert_eq!(w.rows[2].producer, Some((4017, 2, 102)));
    assert!(w.rows[0].transactional);
}

#[test]
fn bytes_that_are_not_batches_end_the_walk_without_a_panic() {
    for seed in 0..500_u32 {
        let junk: Vec<u8> = (0..120_u32)
            .map(|i| (seed.wrapping_mul(7919).wrapping_add(i * 31) >> 3) as u8)
            .collect();
        let _ = walk(&Bytes::from(junk), 0, 0, 10, &[(1, 0)], (KEEP, 10));
    }
    let mut good = batch(0, 3, 0, None).to_vec();
    good.truncate(good.len() - 5);
    assert!(
        walk(&Bytes::from(good), 0, 0, 10, &[], (KEEP, 10))
            .rows
            .is_empty()
    );
}
