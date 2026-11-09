//! Cleaning a log that holds a transaction.

use std::collections::HashMap;

use kafka_wire::batch::Marker;
use kafka_wire::batch::build::{BatchBuilder, Rec};
use kafka_wire::ids::{ProducerEpoch, ProducerId, TimestampMs};

use super::compaction::{PARAMS, append_all, clean, kv, records};
use super::*;
use crate::compact::{AbortedTxn, Params};

/// Offsets 0 and 1 are the records of a transaction of producer 7, 2 is its abort marker, 3 and 4 are plain; then six more
/// plain records, in segments of two.
fn log_with_an_aborted_transaction() -> (PartitionLog<SimFs>, Vec<AbortedTxn>) {
    let (fs, dir, mut log) = fresh(NEVER);
    let txn = |seq: i32| {
        let rec = Rec {
            key: Some(b"t".to_vec()),
            value: Some(b"v".to_vec()),
            ..Rec::default()
        };
        let built = BatchBuilder::from_records(5, &[rec])
            .attributes(0x10)
            .producer(7, 0, seq)
            .build();
        Batch::parse(built).unwrap()
    };
    let (id, epoch, at) = (
        ProducerId::new(7).unwrap(),
        ProducerEpoch::from_wire(0),
        TimestampMs::from_wire(5),
    );
    let marker = Batch::parse(Marker::Abort.batch(id, epoch, 0, at)).unwrap();
    let first = [
        txn(0),
        txn(1),
        marker,
        kv("m1", Some("1")),
        kv("m2", Some("1")),
    ];
    log.append(&first, NOW, TimestampType::Create).unwrap();
    log.sync().unwrap();
    drop(log);
    let seg = kv("a", Some("v")).len() as u64;
    let mut log = open(&fs.process_restart(), &dir, 2 * seg, Trust::Vouched);
    let plain: Vec<_> = (0..6).map(|i| kv(&format!("p{i}"), Some("1"))).collect();
    append_all(&mut log, &plain);
    (
        log,
        vec![AbortedTxn {
            producer: 7,
            first: 0,
            last: 2,
        }],
    )
}

fn more(log: &mut PartitionLog<SimFs>, keys: [&str; 4]) {
    let batches: Vec<_> = keys.iter().map(|k| kv(k, Some("1"))).collect();
    append_all(log, &batches);
}

fn has_offset(log: &PartitionLog<SimFs>, offset: i64) -> bool {
    records(log).iter().any(|(o, _, _)| *o == offset)
}

#[test]
fn the_marker_of_an_aborted_transaction_stays_while_its_records_are_dropped_then_gets_a_time_and_then_goes()
 {
    let (mut log, aborted) = log_with_an_aborted_transaction();
    // Round one: the records go, the marker stays (nothing is said of its time yet).
    clean(&mut log, &PARAMS, aborted.clone(), HashMap::new()).unwrap();
    assert!(
        !records(&log).iter().any(|(_, k, _)| k == "t"),
        "the aborted records are gone"
    );
    assert!(has_offset(&log, 2), "the marker stays");
    // Round two, later: nothing of the transaction is left to drop, so the marker is given its time. (At 1500 a time
    // given in round one, 10 + 1000, would have passed already.)
    more(&mut log, ["q", "r", "s", "u"]);
    let two = Params {
        now_ms: 1_500,
        ..PARAMS
    };
    clean(&mut log, &two, aborted.clone(), HashMap::new()).unwrap();
    assert!(has_offset(&log, 2), "still there for its retention time");
    // Round three, after that time (1500 + 1000): it goes.
    more(&mut log, ["v", "w", "x", "y"]);
    let three = Params {
        now_ms: 5_000,
        ..PARAMS
    };
    clean(&mut log, &three, aborted, HashMap::new()).unwrap();
    assert!(!has_offset(&log, 2), "gone: {:?}", records(&log));
}

#[test]
fn a_marker_goes_in_the_round_whose_time_is_exactly_its_time_and_not_before() {
    let (mut log, aborted) = log_with_an_aborted_transaction();
    clean(&mut log, &PARAMS, aborted.clone(), HashMap::new()).unwrap();
    // Round two gives the marker its time: 1500 + 1000 = 2500.
    more(&mut log, ["q", "r", "s", "u"]);
    let two = Params {
        now_ms: 1_500,
        ..PARAMS
    };
    clean(&mut log, &two, aborted.clone(), HashMap::new()).unwrap();
    more(&mut log, ["v", "w", "x", "y"]);
    let before = Params {
        now_ms: 2_499,
        ..PARAMS
    };
    clean(&mut log, &before, aborted.clone(), HashMap::new()).unwrap();
    assert!(has_offset(&log, 2), "a millisecond early: it stays");
    more(&mut log, ["z1", "z2", "z3", "z4"]);
    let at = Params {
        now_ms: 2_500,
        ..PARAMS
    };
    clean(&mut log, &at, aborted, HashMap::new()).unwrap();
    assert!(
        !has_offset(&log, 2),
        "at its time it goes: {:?}",
        records(&log)
    );
}

/// A one-record batch of a transaction of `producer`.
fn txn_batch(producer: i64, key: &str, value: &str, seq: i32) -> Batch {
    let rec = Rec {
        key: Some(key.as_bytes().to_vec()),
        value: Some(value.as_bytes().to_vec()),
        ..Rec::default()
    };
    let built = BatchBuilder::from_records(5, &[rec])
        .attributes(0x10)
        .producer(producer, 0, seq)
        .build();
    Batch::parse(built).unwrap()
}

fn abort_marker(producer: i64) -> Batch {
    let (id, epoch, at) = (
        ProducerId::new(producer).unwrap(),
        ProducerEpoch::from_wire(0),
        TimestampMs::from_wire(5),
    );
    Batch::parse(Marker::Abort.batch(id, epoch, 0, at)).unwrap()
}

/// `first` is written, the log is closed and opened with two-record segments, and plain records fill the active segment.
fn closed_and_filled(first: &[Batch]) -> PartitionLog<SimFs> {
    let (fs, dir, mut log) = fresh(NEVER);
    log.append(first, NOW, TimestampType::Create).unwrap();
    log.sync().unwrap();
    drop(log);
    let seg = kv("a", Some("v")).len() as u64;
    let mut log = open(&fs.process_restart(), &dir, 2 * seg, Trust::Vouched);
    let plain: Vec<_> = (0..6).map(|i| kv(&format!("p{i}"), Some("1"))).collect();
    append_all(&mut log, &plain);
    log
}

#[test]
fn an_aborted_record_of_a_key_does_not_take_away_the_committed_value_before_it() {
    // 0: k=good, committed; 1: k=bad, of a transaction that 2 aborts.
    let mut log = closed_and_filled(&[
        kv("k", Some("good")),
        txn_batch(7, "k", "bad", 0),
        abort_marker(7),
    ]);
    let aborted = vec![AbortedTxn {
        producer: 7,
        first: 1,
        last: 2,
    }];
    clean(&mut log, &PARAMS, aborted, HashMap::new()).unwrap();
    let after = records(&log);
    assert!(
        after.iter().any(|(o, k, _)| *o == 0 && k == "k"),
        "the committed value of k is gone: {after:?}"
    );
    assert!(
        !after.iter().any(|(o, _, _)| *o == 1),
        "the aborted record stays: {after:?}"
    );
}

#[test]
fn several_aborted_transactions_of_one_key_leave_the_latest_committed_value_of_it() {
    // 0: k=old; 1: k=good (both committed); 2, 4: aborted records of two producers; 3, 5: their markers.
    let mut log = closed_and_filled(&[
        kv("k", Some("old")),
        kv("k", Some("good")),
        txn_batch(7, "k", "bad1", 0),
        abort_marker(7),
        txn_batch(8, "k", "bad2", 0),
        abort_marker(8),
    ]);
    let aborted = vec![
        AbortedTxn {
            producer: 7,
            first: 2,
            last: 3,
        },
        AbortedTxn {
            producer: 8,
            first: 4,
            last: 5,
        },
    ];
    clean(&mut log, &PARAMS, aborted, HashMap::new()).unwrap();
    let after = records(&log);
    let held: Vec<_> = after.iter().filter(|(_, k, _)| k == "k").collect();
    assert_eq!(held.len(), 1, "{after:?}");
    assert_eq!(held[0].0, 1, "the latest committed value, where it was");
}
