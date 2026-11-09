//! Cleaning a partition's log, end to end on the simulated disk: a round is planned from the log, run on its readers, and
//! put in place by the writer.

use std::collections::{BTreeMap, HashMap};

use kafka_wire::batch::Batches;
use kafka_wire::batch::build::{BatchBuilder, Rec};

use super::*;
use crate::compact::{AbortedTxn, Cleaned, LastOf, Params};
use crate::sim::Failure;
use crate::testutil::offset;

pub(super) const PARAMS: Params = Params {
    min_dirty_ratio: 0.0,
    min_lag_ms: 0,
    max_lag_ms: i64::MAX,
    delete_retention_ms: 1_000,
    now_ms: 10,
    map_bytes: 1 << 20,
};

/// A one-record batch for `key`, a deletion if `value` is none.
pub(super) fn kv(key: &str, value: Option<&str>) -> Batch {
    let rec = Rec {
        timestamp_delta: 0,
        offset_delta: None,
        key: Some(key.as_bytes().to_vec()),
        value: value.map(|v| v.as_bytes().to_vec()),
        headers: vec![],
    };
    Batch::parse(BatchBuilder::from_records(5, &[rec]).build()).unwrap()
}

pub(super) fn append_all(log: &mut PartitionLog<SimFs>, batches: &[Batch]) {
    // One at a time: the log rolls between appends, not inside one.
    for b in batches {
        log.append(std::slice::from_ref(b), NOW, TimestampType::Create)
            .unwrap();
    }
    log.sync().unwrap();
}

/// Every record the log holds from its start: (offset, key, is a deletion).
pub(super) fn records(log: &PartitionLog<SimFs>) -> Vec<(i64, String, bool)> {
    let reader = log.reader();
    let mut at = log.start_offset();
    let mut out = Vec::new();
    while at < log.end_offset() {
        let chunk = reader.read(at, 1 << 20, None).unwrap();
        assert!(!chunk.is_empty(), "nothing to read at {at}");
        for batch in Batches::new(chunk) {
            let batch = batch.unwrap();
            batch
                .each_record(1 << 20, |s| {
                    let key = String::from_utf8(s.key.unwrap_or_default().to_vec()).unwrap();
                    out.push((s.offset, key, s.tombstone));
                })
                .unwrap();
            at = batch.next_offset().unwrap();
        }
    }
    out.retain(|(o, _, _)| *o >= log.start_offset().get());
    out
}

/// One round, planned, run and put in place. `None`: the log was not due.
pub(super) fn clean(
    log: &mut PartitionLog<SimFs>,
    params: &Params,
    aborted: Vec<AbortedTxn>,
    producers: HashMap<i64, LastOf>,
) -> Option<Cleaned> {
    let job = log
        .clean_job(params, log.end_offset(), aborted, producers)
        .unwrap()?;
    let cleaned = job.run().unwrap();
    log.install_cleaned(&cleaned).unwrap();
    Some(cleaned)
}

pub(super) fn plain(log: &mut PartitionLog<SimFs>) -> Option<Cleaned> {
    clean(log, &PARAMS, vec![], HashMap::new())
}

/// A log of one-record batches, a segment holding about three of them.
pub(super) fn keyed(keys: &[(&str, Option<&str>)]) -> PartitionLog<SimFs> {
    let (_fs, _dir, mut log) = fresh(3 * kv("a", Some("v")).len() as u64);
    let batches: Vec<_> = keys.iter().map(|(k, v)| kv(k, *v)).collect();
    append_all(&mut log, &batches);
    // The disk handle lives on in the log.
    std::mem::forget(_fs);
    log
}

#[test]
fn only_the_latest_record_of_each_key_is_left_below_the_active_segment_and_offsets_do_not_move() {
    let keys = [
        ("a", Some("1")),
        ("b", Some("1")),
        ("a", Some("2")),
        ("c", Some("1")),
        ("a", Some("3")),
        ("b", Some("2")),
        ("d", Some("1")),
        ("a", Some("4")),
    ];
    let mut log = keyed(&keys);
    let (end, before) = (log.end_offset(), records(&log));
    assert_eq!(before.len(), 8);
    let cleaned = plain(&mut log).expect("due");
    assert!(!cleaned.replacements.is_empty());
    let after = records(&log);
    assert_eq!(log.end_offset(), end, "the end does not move");
    // Every offset left is one that was there, with the same key.
    for r in &after {
        assert!(before.contains(r), "{r:?} was not in the log");
    }
    // The latest of each key is left, wherever it is.
    let mut latest: BTreeMap<String, i64> = BTreeMap::new();
    for (o, k, _) in &before {
        latest.insert(k.clone(), *o);
    }
    for (k, o) in &latest {
        assert!(
            after.iter().any(|(ao, ak, _)| ak == k && ao == o),
            "the latest of {k} is gone"
        );
    }
    assert!(after.len() < before.len(), "something was removed");
    // Below the cleaned part no key is there twice.
    let clean_part: Vec<_> = after
        .iter()
        .filter(|(o, _, _)| *o < cleaned.clean_to)
        .collect();
    let mut seen = std::collections::HashSet::new();
    assert!(clean_part.iter().all(|(_, k, _)| seen.insert(k.clone())));
}

#[test]
fn a_second_round_with_nothing_new_changes_nothing() {
    let mut log = keyed(&[
        ("a", Some("1")),
        ("a", Some("2")),
        ("a", Some("3")),
        ("b", Some("1")),
        ("b", Some("2")),
        ("b", Some("3")),
        ("c", Some("1")),
    ]);
    assert!(plain(&mut log).is_some());
    let once = records(&log);
    // Nothing is dirty now: the log is not due.
    assert!(plain(&mut log).is_none());
    assert_eq!(records(&log), once);
}

#[test]
fn a_deletion_stays_for_the_retention_time_after_it_is_first_cleaned_and_then_goes_with_the_value_it_deleted()
 {
    let mut log = keyed(&[
        ("a", Some("1")),
        ("a", None),
        ("x", Some("1")),
        ("y", Some("1")),
        ("z", Some("1")),
        ("w", Some("1")),
        ("v", Some("1")),
    ]);
    plain(&mut log).unwrap();
    let first = records(&log);
    assert!(
        first.contains(&(1, "a".into(), true)),
        "the deletion stays: {first:?}"
    );
    assert!(
        !first.iter().any(|(o, _, _)| *o == 0),
        "the value it deleted is gone"
    );
    // More arrives, so the log is due again, after the deletion's time has come (horizon 10 + 1000).
    append_all(
        &mut log,
        &[
            kv("p", Some("1")),
            kv("q", Some("1")),
            kv("r", Some("1")),
            kv("s", Some("1")),
        ],
    );
    let later = Params {
        now_ms: 2_000,
        ..PARAMS
    };
    clean(&mut log, &later, vec![], HashMap::new()).unwrap();
    let last = records(&log);
    assert!(
        !last.iter().any(|(_, k, _)| k == "a"),
        "the deletion is gone: {last:?}"
    );
}

#[test]
fn a_segment_all_superseded_keeps_one_empty_batch_so_the_offsets_after_it_are_still_reached() {
    let mut log = keyed(&[
        ("a", Some("1")),
        ("a", Some("2")),
        ("a", Some("3")),
        ("a", Some("4")),
        ("a", Some("5")),
        ("a", Some("6")),
        ("b", Some("1")),
    ]);
    plain(&mut log).unwrap();
    let after = records(&log);
    assert_eq!(after.iter().map(|(o, _, _)| *o).collect::<Vec<_>>(), [5, 6]);
    // Read from inside the removed part: the next record is the answer, and the log can be opened again.
    let from_gap = log.reader().read(offset(1), 1 << 20, None).unwrap();
    assert!(!from_gap.is_empty());
}

#[test]
fn what_cleaning_left_is_what_opening_the_log_again_finds() {
    let (fs, dir, mut log) = fresh(3 * kv("a", Some("v")).len() as u64);
    let keys: Vec<_> = (0..12)
        .map(|i| kv(&format!("k{}", i % 4), Some("v")))
        .collect();
    append_all(&mut log, &keys);
    plain(&mut log).unwrap();
    let (kept, end) = (records(&log), log.end_offset());
    drop(log);
    let again = open(
        &fs.process_restart(),
        &dir,
        3 * kv("a", Some("v")).len() as u64,
        Trust::Vouched,
    );
    assert_eq!((records(&again), again.end_offset()), (kept, end));
}

#[test]
fn a_segment_retention_removed_while_a_round_ran_is_not_put_back_and_leaves_no_files() {
    let (fs, dir, mut log) = fresh(3 * kv("a", Some("v")).len() as u64);
    let batches: Vec<_> = (0..12)
        .map(|i| kv(&format!("k{}", i % 3), Some("v")))
        .collect();
    append_all(&mut log, &batches);
    let job = log
        .clean_job(&PARAMS, log.end_offset(), vec![], HashMap::new())
        .unwrap()
        .unwrap();
    let cleaned = job.run().unwrap();
    assert!(cleaned.replacements.len() >= 2);
    // The oldest segment goes (DeleteRecords past it) before the round is put in place.
    let gone = cleaned.replacements.first().unwrap().base;
    log.delete_before(Some(offset(3))).unwrap();
    let put = log.install_cleaned(&cleaned).unwrap();
    assert_eq!(put, cleaned.replacements.len() - 1);
    let names = fs.names(&dir);
    assert!(!names.iter().any(|n| n.ends_with(".tmp")), "{names:?}");
    assert!(
        !names
            .iter()
            .any(|n| n.starts_with(&format!("{:020}", gone.get()))),
        "{names:?}"
    );
}

#[test]
fn a_round_put_in_place_twice_changes_nothing_the_second_time_and_leaves_no_files() {
    let (fs, dir, mut log) = fresh(3 * kv("a", Some("v")).len() as u64);
    let batches: Vec<_> = (0..12)
        .map(|i| kv(&format!("k{}", i % 3), Some("v")))
        .collect();
    append_all(&mut log, &batches);
    let job = log
        .clean_job(&PARAMS, log.end_offset(), vec![], HashMap::new())
        .unwrap()
        .unwrap();
    let cleaned = job.run().unwrap();
    assert!(log.install_cleaned(&cleaned).unwrap() > 0);
    let once = records(&log);
    assert_eq!(
        log.install_cleaned(&cleaned).unwrap(),
        0,
        "those segments are not as the round found them"
    );
    assert_eq!(records(&log), once);
    assert!(!fs.names(&dir).iter().any(|n| n.ends_with(".tmp")));
}

/// A one-record batch stamped `ts`.
fn kv_at(key: &str, value: &str, ts: i64) -> Batch {
    let rec = Rec {
        key: Some(key.as_bytes().to_vec()),
        value: Some(value.as_bytes().to_vec()),
        ..Rec::default()
    };
    Batch::parse(BatchBuilder::from_records(ts, &[rec]).build()).unwrap()
}

#[test]
fn the_newest_timestamp_is_of_a_record_that_is_left_not_of_a_batch_cleaning_has_emptied() {
    let seg = kv("a", Some("v")).len() as u64;
    let (_fs, _dir, mut log) = fresh(3 * seg);
    // Offset 2 has the newest timestamp and is superseded by offset 3 (older), where it ends its segment and stays as an
    // empty batch with its header as it was.
    append_all(
        &mut log,
        &[
            kv_at("b", "1", 200),
            kv_at("c", "0", 150),
            kv_at("a", "1", 9_000),
            kv_at("a", "2", 100),
            kv_at("c", "1", 300),
            kv_at("d", "1", 400),
            kv_at("e", "1", 500),
            kv_at("f", "1", 600),
        ],
    );
    assert_eq!(
        log.reader().offset_of_max_timestamp().unwrap().map(|(o, t)| (o.get(), t.get())),
        Some((2, 9_000))
    );
    plain(&mut log).expect("due");
    assert_eq!(
        log.reader().offset_of_max_timestamp().unwrap().map(|(o, t)| (o.get(), t.get())),
        Some((7, 600)),
        "the records left: {:?}",
        records(&log)
    );
}

#[test]
fn a_checkpoint_beyond_the_end_of_the_log_is_begun_again_from_the_start() {
    let seg = kv("a", Some("v")).len() as u64;
    let (fs, dir, mut log) = fresh(3 * seg);
    let batches: Vec<_> = (0..60).map(|i| kv(&format!("k{}", i % 4), Some("v"))).collect();
    append_all(&mut log, &batches);
    crate::fs::write_durably(&fs, &dir.join("cleaner-checkpoint"), b"5000\n").unwrap();
    let before = records(&log).len();
    plain(&mut log).expect("a checkpoint past the end does not stop cleaning");
    assert!(records(&log).len() < before);
}

#[test]
fn a_stale_temporary_file_of_an_earlier_round_does_not_get_into_a_new_one() {
    let seg = kv("a", Some("v")).len() as u64;
    let (fs, dir, mut log) = fresh(3 * seg);
    let batches: Vec<_> = (0..30).map(|i| kv(&format!("k{}", i % 3), Some("v"))).collect();
    append_all(&mut log, &batches);
    let want = {
        let (fs2, dir2, mut twin) = fresh(3 * seg);
        append_all(&mut twin, &batches);
        plain(&mut twin).expect("due");
        std::mem::forget((fs2, dir2));
        records(&twin)
    };
    // What a round that was stopped left behind: longer than the file it would make, and not a log at all.
    crate::fs::write_durably(&fs, &dir.join("00000000000000000000.log.tmp"), &[0xAB; 4096]).unwrap();
    plain(&mut log).expect("due");
    assert_eq!(records(&log), want);
    assert!(
        fs.names(&dir).iter().all(|n| !n.ends_with(".tmp")),
        "no temporary file is left: {:?}",
        fs.names(&dir)
    );
}

#[test]
fn a_round_that_cannot_read_a_segment_leaves_no_temporary_file() {
    let seg = kv("a", Some("v")).len() as u64;
    let (fs, dir, mut log) = fresh(3 * seg);
    let batches: Vec<_> = (0..30).map(|i| kv(&format!("k{}", i % 3), Some("v"))).collect();
    append_all(&mut log, &batches);
    let job = log
        .clean_job(&PARAMS, log.end_offset(), vec![], HashMap::new())
        .unwrap()
        .expect("due");
    // The map is read first; the walk that writes comes after, and fails on its first read.
    fs.fail(Op::Read, 40, Failure::Io);
    let outcome = job.run();
    fs.clear_faults();
    assert!(outcome.is_err(), "the round did not fail");
    assert!(
        fs.names(&dir).iter().all(|n| !n.ends_with(".tmp")),
        "a failed round leaves nothing: {:?}",
        fs.names(&dir)
    );
}

#[test]
fn a_segment_that_cannot_be_opened_again_after_it_was_put_in_place_does_not_take_the_partition_offline() {
    let seg = kv("a", Some("v")).len() as u64;
    let (fs, dir, mut log) = fresh(3 * seg);
    let batches: Vec<_> = (0..30).map(|i| kv(&format!("k{}", i % 3), Some("v"))).collect();
    append_all(&mut log, &batches);
    let job = log
        .clean_job(&PARAMS, log.end_offset(), vec![], HashMap::new())
        .unwrap()
        .expect("due");
    let cleaned = job.run().unwrap();
    // The files are swapped; opening the new segment fails for want of a descriptor (it may well work a moment later).
    fs.fail(Op::Read, 1, Failure::TooManyFiles);
    assert!(log.install_cleaned(&cleaned).is_err());
    fs.clear_faults();
    log.append(&[kv("k0", Some("after"))], NOW, TimestampType::Create)
        .expect("the partition is still online");
    log.sync().unwrap();
    // What is on disk is whole, and is what a restart finds.
    drop(log);
    let again = reopen(&fs.process_restart(), &dir);
    let latest: Vec<_> = records(&again).into_iter().filter(|r| r.1 == "k0").collect();
    assert_eq!(latest.last().map(|r| r.0), Some(30), "{latest:?}");
}
