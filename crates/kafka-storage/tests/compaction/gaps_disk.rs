//! Rounds that remove segments, against a disk that stops or fails and against readers on other threads.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use kafka_storage::sim::{Cut, Failure, SimFs};
use kafka_storage::{StorageError, Trust};
use kafka_wire::batch::{Batches, TimestampType};

use crate::gaps::{files, one};
use crate::sweeps::endings;
use crate::world::{NOW, PARAMS, Rng, Seen, clean_fully, holds, offset, open, records, written};

/// A round on a written workload, stopped after `stop` file operations; what the disk then holds.
fn stopped(
    workload: (u64, usize, u64),
    stop: u64,
    end: Option<Cut>,
) -> (SimFs, PathBuf, Vec<Seen>) {
    let (seed, appends, keys) = workload;
    let (fs, dir, truth) = written(seed, appends, keys);
    let mut log = open(&fs, &dir, Trust::Vouched);
    fs.halt_after(stop);
    let job = log.clean_job(&PARAMS, log.end_offset(), vec![], HashMap::new());
    if let Some(cleaned) = job.ok().flatten().and_then(|job| job.run().ok()) {
        let _ = log.install_cleaned(&cleaned);
    }
    drop(log);
    let disk = end.map_or_else(|| fs.process_restart(), |cut| fs.power_cut(cut));
    (disk, dir, truth)
}

/// A hot key is rewritten until whole segments are left with nothing, so rounds remove files as well as rewrite them. A
/// stop after each step of the round, ended by a few of the ways the process or the power can go (all of them at the
/// step's turn would take minutes), must leave a log that reads, and that cleaned again is what an uninterrupted one is.
#[test]
fn a_stop_in_a_round_that_removes_segments_leaves_a_log_that_cleans_to_the_same_end() {
    for workload in [(5u64, 60usize, 2u64), (11, 50, 1), (17, 70, 3)] {
        let (seed, appends, keys) = workload;
        let (fs, dir, truth) = written(seed, appends, keys);
        let mut log = open(&fs, &dir, Trust::Vouched);
        let segments_before = files(&fs, &dir).len();
        let before = fs.ops();
        clean_fully(&mut log);
        let steps = fs.ops() - before;
        let want = records(&log);
        let segments_after = files(&fs, &dir).len();
        assert!(segments_after < segments_before, "no segment removed");
        for stop in 0..=steps {
            let all = endings(stop);
            let (a, b) = (1 + stop as usize % 10, 11 + stop as usize % 16);
            for end in [all[0], all[a], all[b]] {
                let what = format!("seed {seed}, stop {stop}/{steps}, {end:?}");
                let (disk, dir, _) = stopped(workload, stop, end);
                let log = open(&disk, &dir, Trust::Unvouched);
                holds(&log, &truth, &what);
                drop(log);
                let disk = disk.process_restart();
                let mut log = open(&disk, &dir, Trust::Unvouched);
                holds(&log, &truth, &format!("{what}, reopened"));
                clean_fully(&mut log);
                holds(&log, &truth, &format!("{what}, cleaned"));
                assert_eq!(
                    records(&log),
                    want,
                    "{what}: not what an uninterrupted round leaves"
                );
                // The files too: no emptied segment is left behind but the first.
                let n = files(&disk, &dir).len();
                assert!(
                    n <= segments_after,
                    "{what}: {n} files, uninterrupted {segments_after}"
                );
            }
        }
    }
}

/// The base offsets of the temporary files in a directory.
fn temporary_bases(fs: &SimFs, dir: &std::path::Path) -> HashSet<String> {
    fs.names(dir)
        .into_iter()
        .filter(|n| n.contains(".tmp"))
        .map(|n| n.chars().take(20).collect())
        .collect()
}

/// A file operation fails at each point of putting a round in place, as an I/O error and as no descriptor to open the new
/// segment with. The files of the replacements that are not reached are taken back by the call itself, so that at most the
/// one it failed on can have files left; and the log is whole, writable and cleans to the same end.
#[test]
fn a_failed_file_operation_in_putting_a_round_in_place_takes_back_the_rest_and_leaves_a_whole_log()
{
    for workload in [(5u64, 60usize, 2u64), (17, 70, 3)] {
        let (seed, appends, keys) = workload;
        let (fs0, dir0, truth) = written(seed, appends, keys);
        let mut uninterrupted = open(&fs0, &dir0, Trust::Vouched);
        clean_fully(&mut uninterrupted);
        let want = records(&uninterrupted);
        let mut failures = 0;
        for nth in 1..=140u64 {
            for failure in [Failure::Io, Failure::TooManyFiles] {
                let (fs, dir, _) = written(seed, appends, keys);
                let mut log = open(&fs, &dir, Trust::Vouched);
                let job = log
                    .clean_job(&PARAMS, log.end_offset(), vec![], HashMap::new())
                    .unwrap()
                    .expect("due");
                let cleaned = job.run().unwrap();
                assert!(
                    cleaned.replacements.len() >= 3,
                    "a round of {}",
                    cleaned.replacements.len()
                );
                fs.fail_any(nth, failure);
                let outcome = log.install_cleaned(&cleaned);
                fs.clear_faults();
                let what = format!(
                    "seed {seed}, op {nth}, {failure:?}, installed {}",
                    outcome.is_ok()
                );
                if outcome.is_err() {
                    failures += 1;
                    let left = temporary_bases(&fs, &dir);
                    assert!(
                        left.len() <= 1,
                        "{what}: files of several replacements left: {left:?}"
                    );
                    log.discard_cleaned(&cleaned);
                }
                assert!(temporary_bases(&fs, &dir).is_empty(), "{what}");
                holds(&log, &truth, &what);
                assert!(!log.is_offline(), "{what}: offline");
                // A restart finds a whole log, and cleaning it again ends where an uninterrupted round did.
                drop(log);
                let disk = fs.process_restart();
                let mut log = open(&disk, &dir, Trust::Unvouched);
                holds(&log, &truth, &format!("{what}, reopened"));
                clean_fully(&mut log);
                holds(&log, &truth, &format!("{what}, cleaned"));
                assert_eq!(records(&log), want, "{what}: re-cleaned log differs");
                assert!(
                    disk.names(&dir).iter().all(|n| !n.ends_with(".tmp")),
                    "{what}: temporary files left {:?}",
                    disk.names(&dir)
                );
            }
        }
        assert!(failures > 20, "only {failures} installs failed");
    }
}

/// Readers go on reading whole batches while the writer appends, cleans, removes segments and trims.
#[test]
fn readers_on_other_threads_see_a_whole_log_while_segments_are_cleaned_and_removed() {
    let fs = SimFs::new();
    let dir = PathBuf::from("/data/t-0");
    kafka_storage::fs::create_dirs_durably(&fs, &dir).unwrap();
    let mut log = open(&fs, &dir, Trust::Unvouched);
    let reader = log.reader();
    let stop = Arc::new(AtomicBool::new(false));
    let mut threads = Vec::new();
    for t in 0..3u64 {
        let (reader, stop) = (reader.clone(), Arc::clone(&stop));
        threads.push(std::thread::spawn(move || {
            let mut rng = Rng::new(100 + t);
            let mut reads = 0u64;
            while !stop.load(Ordering::Relaxed) {
                let (s, e) = (reader.start_offset().get(), reader.end_offset().get());
                if e == s {
                    continue;
                }
                let from = s + i64::try_from(rng.below(u64::try_from(e - s).unwrap())).unwrap();
                match reader.read(offset(from), 1 << 16, None) {
                    Ok(bytes) => {
                        for b in Batches::new(bytes) {
                            b.unwrap_or_else(|err| {
                                panic!("reader {t}: bad batch from {from}: {err}")
                            });
                        }
                    }
                    Err(StorageError::OutOfRange { .. }) => {}
                    Err(err) => panic!("reader {t}: read from {from} failed: {err}"),
                }
                reads += 1;
            }
            reads
        }));
    }
    for round in 0..40 {
        for i in 0..30 {
            let ts = 1000 + i64::from(round * 30 + i);
            let batch = one("hot", ts, Some("vvvvvvvvvvvvvvvvvvvv"));
            log.append(&[batch], NOW, TimestampType::Create).unwrap();
        }
        log.sync().unwrap();
        clean_fully(&mut log);
        if round % 7 == 6 {
            let to = offset(log.end_offset().get() - 20);
            log.delete_before(Some(to)).unwrap();
        }
    }
    stop.store(true, Ordering::Relaxed);
    let reads: u64 = threads.into_iter().map(|t| t.join().unwrap()).sum();
    assert!(reads > 0, "the readers read nothing");
    assert!(
        files(&fs, &dir).len() < 20,
        "segments pile up: {:?}",
        files(&fs, &dir)
    );
}
