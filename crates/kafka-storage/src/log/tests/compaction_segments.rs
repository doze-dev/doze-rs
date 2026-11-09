//! Cleaning removes a segment it leaves with nothing the log needs, so the files do not outnumber the data (N3 of the
//! Phase 7 audit: a hot key left one file for every segment it ever filled).

use std::collections::HashMap;
use std::path::Path;

use kafka_wire::batch::build::{BatchBuilder, Rec};

use super::compaction::{PARAMS, append_all, clean, kv, plain, records};
use super::*;
use crate::compact::LastOf;

/// The size of a segment: two of the one-record batches below.
fn two() -> u64 {
    2 * kv("hot", Some("00")).len() as u64
}

/// `n` records of one key, two to a segment.
fn hot(n: usize) -> (SimFs, PathBuf, PartitionLog<SimFs>) {
    let (fs, dir, mut log) = fresh(two());
    let batches: Vec<_> = (0..n)
        .map(|i| kv("hot", Some(&format!("{i:02}"))))
        .collect();
    append_all(&mut log, &batches);
    (fs, dir, log)
}

fn log_files(fs: &SimFs, dir: &Path) -> usize {
    fs.names(dir).iter().filter(|n| n.ends_with(".log")).count()
}

#[test]
fn a_hot_key_leaves_a_few_segments_and_not_one_for_each_it_filled() {
    let (fs, dir, mut log) = hot(60);
    assert_eq!(log_files(&fs, &dir), 30);
    plain(&mut log).expect("due");
    // The first segment (where the log starts), the one holding the last record the map covered, and the active one.
    assert_eq!(log_files(&fs, &dir), 3, "{:?}", fs.names(&dir));
    assert_eq!(log.start_offset().get(), 0, "the start does not move");
    assert_eq!(log.end_offset().get(), 60, "nor the end");
    let left = records(&log);
    assert_eq!(left.last().map(|(o, _, _)| *o), Some(59));
    assert!(left.len() <= 3, "{left:?}");
    assert!(
        !fs.names(&dir).iter().any(|n| n.ends_with(".tmp")),
        "no temporary file is left"
    );
}

#[test]
fn what_is_left_after_segments_vanish_is_what_opening_the_log_again_finds_and_the_log_goes_on() {
    let (fs, dir, mut log) = hot(60);
    plain(&mut log).expect("due");
    let (kept, end) = (records(&log), log.end_offset());
    drop(log);
    let mut again = open(&fs.process_restart(), &dir, two(), Trust::Vouched);
    assert_eq!((records(&again), again.end_offset()), (kept, end));
    // Appending after the gaps reaches the new record, and the offsets go on from the end.
    append_all(&mut again, &[kv("hot", Some("60"))]);
    assert_eq!(again.end_offset().get(), 61);
    assert_eq!(records(&again).last().map(|(o, _, _)| *o), Some(60));
}

#[test]
fn rounds_one_after_another_do_not_let_the_files_grow() {
    let (fs, dir, mut log) = hot(20);
    let mut most = 0;
    for round in 0..5 {
        plain(&mut log);
        append_all(
            &mut log,
            &(0..20)
                .map(|i| kv("hot", Some(&format!("{round}{i:02}"))))
                .collect::<Vec<_>>(),
        );
        plain(&mut log);
        most = most.max(log_files(&fs, &dir));
    }
    assert!(most <= 4, "the files grew to {most}");
    assert_eq!(records(&log).last().map(|(o, _, _)| *o), Some(119));
}

#[test]
fn the_first_segment_is_never_taken() {
    let (_fs, _dir, mut log) = hot(60);
    plain(&mut log).expect("due");
    let first = log.sealed.first().expect("a sealed segment");
    assert_eq!(first.base().get(), 0, "the log still begins where it did");
    // Reading from the start finds the next record, past the gap.
    let from_start = log
        .reader()
        .read(log.start_offset(), 1 << 20, None)
        .unwrap();
    assert!(!from_start.is_empty());
    std::mem::forget(_fs);
}

/// Six batches of `hot`, then the producer 9 writes `hot` as its only batch (offset 6 or so), then more of `hot`.
fn with_a_producer() -> PartitionLog<SimFs> {
    let (fs, _dir, mut log) = fresh(two());
    let from = |producer: Option<i64>, v: &str| {
        let rec = Rec {
            key: Some(b"hot".to_vec()),
            value: Some(v.as_bytes().to_vec()),
            ..Rec::default()
        };
        let mut b = BatchBuilder::from_records(5, &[rec]);
        if let Some(p) = producer {
            b = b.producer(p, 0, 0);
        }
        Batch::parse(b.build()).unwrap()
    };
    let mut batches: Vec<_> = (0..6).map(|i| from(None, &format!("{i:02}"))).collect();
    batches.push(from(Some(9), "09"));
    batches.extend((10..20).map(|i| from(None, &format!("{i:02}"))));
    append_all(&mut log, &batches);
    std::mem::forget(fs);
    log
}

#[test]
fn a_segment_holding_the_last_batch_of_a_producer_stays_though_its_records_are_gone() {
    let mut without = with_a_producer();
    clean(&mut without, &PARAMS, vec![], HashMap::new()).expect("due");
    let mut with = with_a_producer();
    let producers = HashMap::from([(
        9,
        LastOf {
            data_last_offset: Some(6),
            marker_epoch: -1,
        },
    )]);
    clean(&mut with, &PARAMS, vec![], producers).expect("due");
    assert_eq!(
        with.sealed.len(),
        without.sealed.len() + 1,
        "the producer's batch keeps its segment, so the broker can still tell its sequence"
    );
    assert_eq!(
        records(&with),
        records(&without),
        "the records are the same"
    );
}

#[test]
fn a_round_whose_dirty_part_is_a_single_offset_still_cleans() {
    // A segment holds one batch. Round one cleans offsets 0 and 1 (offset 2 is in the active segment).
    let one = kv("hot", Some("00")).len() as u64;
    let (_fs, _dir, mut log) = fresh(one);
    let first: Vec<_> = (0..3)
        .map(|i| kv("hot", Some(&format!("{i:02}"))))
        .collect();
    append_all(&mut log, &first);
    assert_eq!(plain(&mut log).expect("due").clean_to, 2);
    // One more batch seals offset 2: that is all that is dirty now, and it supersedes offset 1.
    append_all(&mut log, &[kv("hot", Some("03"))]);
    assert_eq!(plain(&mut log).expect("due").clean_to, 3);
    let left: Vec<i64> = records(&log).iter().map(|r| r.0).collect();
    assert_eq!(left, [2, 3]);
    std::mem::forget(_fs);
}
