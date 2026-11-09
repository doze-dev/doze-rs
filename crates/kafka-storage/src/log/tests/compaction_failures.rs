//! A round that fails, is stopped, or meets files an earlier one left: what it leaves on disk, and the log as it was.

use std::collections::HashMap;

use super::compaction::{PARAMS, append_all, kv, plain, records};
use super::*;
use crate::sim::Failure;

#[test]
fn a_stale_temporary_file_of_an_earlier_round_does_not_get_into_a_new_one() {
    let seg = kv("a", Some("v")).len() as u64;
    let (fs, dir, mut log) = fresh(3 * seg);
    let batches: Vec<_> = (0..30)
        .map(|i| kv(&format!("k{}", i % 3), Some("v")))
        .collect();
    append_all(&mut log, &batches);
    let want = {
        let (fs2, dir2, mut twin) = fresh(3 * seg);
        append_all(&mut twin, &batches);
        plain(&mut twin).expect("due");
        std::mem::forget((fs2, dir2));
        records(&twin)
    };
    // What a round that was stopped left behind: longer than the file it would make, and not a log at all.
    crate::fs::write_durably(
        &fs,
        &dir.join("00000000000000000000.log.tmp"),
        &[0xAB; 4096],
    )
    .unwrap();
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
    // A read fails at each point of the round in turn: the map is read first, the walk that writes comes after.
    let mut failed = 0;
    for nth in (1..=60).step_by(3) {
        let (fs, dir, mut log) = fresh(3 * seg);
        let batches: Vec<_> = (0..30)
            .map(|i| kv(&format!("k{}", i % 3), Some("v")))
            .collect();
        append_all(&mut log, &batches);
        let job = log
            .clean_job(&PARAMS, log.end_offset(), vec![], HashMap::new())
            .unwrap()
            .expect("due");
        fs.fail(Op::Read, nth, Failure::Io);
        let outcome = job.run();
        fs.clear_faults();
        // A round that did not meet the fault is whole, and its files wait to be put in place.
        if outcome.is_err() {
            failed += 1;
            assert!(
                fs.names(&dir).iter().all(|n| !n.ends_with(".tmp")),
                "a failed round leaves nothing (read {nth}): {:?}",
                fs.names(&dir)
            );
        }
    }
    assert!(failed >= 5, "only {failed} rounds failed");
}

#[test]
fn a_segment_that_cannot_be_opened_again_after_it_was_put_in_place_does_not_take_the_partition_offline()
 {
    let seg = kv("a", Some("v")).len() as u64;
    let (fs, dir, mut log) = fresh(3 * seg);
    let batches: Vec<_> = (0..30)
        .map(|i| kv(&format!("k{}", i % 3), Some("v")))
        .collect();
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
    let latest: Vec<_> = records(&again)
        .into_iter()
        .filter(|r| r.1 == "k0")
        .collect();
    assert_eq!(latest.last().map(|r| r.0), Some(30), "{latest:?}");
}

#[test]
fn a_round_told_to_stop_gives_up_at_once_and_leaves_no_files_and_the_log_as_it_was() {
    use std::sync::atomic::AtomicBool;
    let seg = kv("a", Some("v")).len() as u64;
    let (fs, dir, mut log) = fresh(3 * seg);
    let batches: Vec<_> = (0..30)
        .map(|i| kv(&format!("k{}", i % 3), Some("v")))
        .collect();
    append_all(&mut log, &batches);
    let before = records(&log);
    let job = log
        .clean_job(&PARAMS, log.end_offset(), vec![], HashMap::new())
        .unwrap()
        .expect("due");
    let stop = AtomicBool::new(true);
    let outcome = job.run_until(&stop);
    match outcome {
        Err(crate::StorageError::Io { source, .. }) => {
            assert_eq!(source.kind(), std::io::ErrorKind::Interrupted);
        }
        other => panic!("not interrupted: {other:?}"),
    }
    assert!(fs.names(&dir).iter().all(|n| !n.ends_with(".tmp")));
    assert_eq!(records(&log), before);
}
