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
