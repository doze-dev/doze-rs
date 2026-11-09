//! Which batch cleaning keeps whole for a producer, though cleaning empties it: the producer's last (what its state
//! needs to recognise a retry, or the marker that ends its transaction).

use std::collections::HashMap;

use kafka_wire::batch::build::{BatchBuilder, Rec};

use super::compaction::{PARAMS, append_all, clean, kv};
use super::*;
use crate::compact::LastOf;

/// The segment of the producer's batch (offset 1).
const ITS_SEGMENT: &str = "00000000000000000001.log";

/// A data batch of producer 7 at epoch 1 with one record of key "hot".
fn data(value: &str) -> Batch {
    let rec = Rec {
        key: Some(b"hot".to_vec()),
        value: Some(value.as_bytes().to_vec()),
        ..Rec::default()
    };
    Batch::parse(
        BatchBuilder::from_records(5, &[rec])
            .producer(7, 1, 0)
            .build(),
    )
    .unwrap()
}

/// What is left of the files after a round over: 0 a record of "hot", 1 the producer's data batch (also "hot"), 2 more
/// of "hot", and so on, a segment to each batch. The producer's state is `last`.
fn after_a_round(last: LastOf) -> Vec<String> {
    let (fs, dir, mut log) = fresh(1);
    let mut batches = vec![kv("hot", Some("0")), data("1")];
    batches.extend((2..6).map(|i| kv("hot", Some(&i.to_string()))));
    append_all(&mut log, &batches);
    clean(&mut log, &PARAMS, vec![], HashMap::from([(7, last)])).expect("due");
    fs.names(&dir)
}

#[test]
fn the_batch_a_producer_last_wrote_data_in_stays_though_its_record_is_superseded() {
    let kept = after_a_round(LastOf {
        data_last_offset: Some(1),
        marker_epoch: 0,
    });
    assert!(kept.contains(&ITS_SEGMENT.to_string()), "{kept:?}");
}

#[test]
fn a_data_batch_is_not_the_marker_of_a_producer_that_has_no_data_though_the_epoch_is_the_same() {
    // The producer's state holds no data of it, and a marker at epoch 1. Its data batch at that epoch is not that
    // marker: with its record superseded it goes, and its segment with it.
    let kept = after_a_round(LastOf {
        data_last_offset: None,
        marker_epoch: 1,
    });
    assert!(!kept.contains(&ITS_SEGMENT.to_string()), "{kept:?}");
}
