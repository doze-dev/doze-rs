//! Cleaning at the edges the plan and the producers' state set: the clock of the minimum lag, and a producer's markers.

use std::collections::HashMap;

use kafka_wire::batch::Marker;
use kafka_wire::ids::{ProducerEpoch, ProducerId};

use super::compaction::{PARAMS, append_all, clean, kv, kv_at, records};
use super::*;
use crate::compact::{LastOf, Params};

/// Six records of one key, stamped `ts`, three to a segment.
fn hot_at(ts: i64) -> PartitionLog<SimFs> {
    let (fs, _dir, mut log) = fresh(3 * kv("hot", Some("v")).len() as u64);
    let batches: Vec<_> = (0..7).map(|i| kv_at("hot", &format!("{i}"), ts)).collect();
    append_all(&mut log, &batches);
    std::mem::forget(fs);
    log
}

#[test]
fn a_record_younger_than_the_minimum_lag_is_not_cleaned_and_an_older_one_is() {
    let lag = Params {
        min_lag_ms: 100,
        now_ms: 10_000,
        ..PARAMS
    };
    // Stamped 50 ms before now: too new. The clock is the records' own, not the files'.
    let mut young = hot_at(9_950);
    assert!(clean(&mut young, &lag, vec![], HashMap::new()).is_none());
    let mut old = hot_at(5);
    assert!(clean(&mut old, &lag, vec![], HashMap::new()).is_some());
}

#[test]
fn of_a_producers_markers_only_the_one_at_its_epoch_stays_once_cleaning_empties_them() {
    let (fs, dir, mut log) = fresh(1);
    let id = ProducerId::new(7).unwrap();
    let at = TimestampMs::from_wire(5);
    let marker = |epoch: i16| {
        let built = Marker::Commit.batch(id, ProducerEpoch::from_wire(epoch), 0, at);
        Batch::parse(built).unwrap()
    };
    // Offsets: 0 a record, 1 the marker of epoch 0, 2 the marker of epoch 1, then plain records; one to a segment.
    let mut batches = vec![kv("hot", Some("0")), marker(0), marker(1)];
    batches.extend((1..5).map(|i| kv("hot", Some(&i.to_string()))));
    append_all(&mut log, &batches);
    let producers = HashMap::from([(
        7,
        LastOf {
            data_last_offset: None,
            marker_epoch: 1,
        },
    )]);
    // Round one gives the markers their time (10 + 1000); round two, after it, empties them.
    clean(&mut log, &PARAMS, vec![], producers.clone()).expect("due");
    append_all(&mut log, &[kv("hot", Some("5")), kv("hot", Some("6"))]);
    let late = Params {
        now_ms: 5_000,
        ..PARAMS
    };
    clean(&mut log, &late, vec![], producers).expect("due");
    let names = fs.names(&dir);
    assert!(
        names.contains(&"00000000000000000002.log".to_string()),
        "the marker of the producer's epoch stays: {names:?}"
    );
    assert!(
        !names.contains(&"00000000000000000001.log".to_string()),
        "an older marker of it goes: {names:?}"
    );
    assert_eq!(records(&log).last().map(|r| r.0), Some(8));
}
