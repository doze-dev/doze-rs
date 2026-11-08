use super::*;
use crate::error::StorageError;
use crate::sim::{Cut, DataCut, Failure, NameCut, Op};
use crate::testutil::offset;

#[test]
fn delete_before_moves_the_start_and_removes_whole_segments_below_it() {
    let (fs, dir, mut log) = three_segments(0);
    assert_eq!(log.delete_before(Some(offset(3))).unwrap(), offset(3));
    assert_eq!(log.start_offset(), offset(3));
    assert_eq!(
        names(&fs, &dir).len(),
        2,
        "segment 0 is gone; the one holding the start stays: {:?}",
        names(&fs, &dir)
    );
    let r = log.reader();
    let err = r.read(offset(2), 100, None).unwrap_err();
    assert!(matches!(
        err,
        StorageError::OutOfRange {
            requested: 2,
            start: 3,
            end: 6
        }
    ));
    assert_eq!(
        bases(&r.read(offset(3), usize::MAX, None).unwrap()),
        vec![3],
        "a read is served from one segment"
    );
    assert_eq!(bases(&all(&r)), vec![3, 4, 5]);
    // At or below the start is a no-op, above the end is out of range.
    assert_eq!(log.delete_before(Some(offset(1))).unwrap(), offset(3));
    assert_eq!(log.delete_before(Some(offset(3))).unwrap(), offset(3));
    assert!(matches!(
        log.delete_before(Some(offset(7))).unwrap_err(),
        StorageError::OutOfRange { .. }
    ));
}

#[test]
fn the_new_start_survives_a_restart() {
    let (fs, dir, mut log) = three_segments(0);
    log.delete_before(Some(offset(3))).unwrap();
    log.sync().unwrap();
    drop(log);
    let log = open(&fs.process_restart(), &dir, 2 * one(), Trust::Unvouched);
    assert_eq!(
        (log.start_offset(), log.end_offset()),
        (offset(3), offset(6))
    );
}

#[test]
fn deleting_to_the_end_empties_the_partition_and_starts_a_fresh_segment() {
    let (fs, dir, mut log) = fresh(NEVER);
    log.append(&ones(4, 0), NOW, TimestampType::Create).unwrap();
    assert_eq!(log.delete_before(None).unwrap(), offset(4));
    assert_eq!(
        (log.start_offset(), log.end_offset()),
        (offset(4), offset(4))
    );
    assert_eq!(
        names(&fs, &dir),
        vec!["00000000000000000004.log"],
        "the old segment does not wait to roll"
    );
    assert!(log.reader().read(offset(4), 100, None).unwrap().is_empty());
    assert_eq!(
        log.append(&ones(1, 0), NOW, TimestampType::Create)
            .unwrap()
            .first_offset,
        offset(4)
    );
    drop(log);
    let log = open(&fs.process_restart(), &dir, NEVER, Trust::Unvouched);
    assert_eq!(
        (log.start_offset(), log.end_offset()),
        (offset(4), offset(5))
    );
}

#[test]
fn the_start_is_recorded_before_any_file_is_removed() {
    let (fs, dir, mut log) = three_segments(0);
    log.sync().unwrap();
    // Stop the process after the start is recorded (a temporary file, its flush, the rename, the directory
    // flush: four operations) and before the first removal.
    fs.halt_after(5);
    let _ = log.delete_before(Some(offset(5)));
    let after = fs.power_cut(Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    });
    let reopened = open(&after, &dir, 2 * one(), Trust::Unvouched);
    assert!(reopened.start_offset() >= offset(0));
    // Whatever the stop caught, the log that opens is consistent: start <= end and all of it reads.
    let r = reopened.reader();
    assert!(reopened.start_offset() <= reopened.end_offset());
    r.read(reopened.start_offset(), usize::MAX, None).unwrap();
}

#[test]
fn deleting_to_the_end_starts_by_flushing_and_a_failure_there_changes_nothing() {
    let (fs, dir, mut log) = fresh(NEVER);
    log.append(&ones(4, 0), NOW, TimestampType::Create).unwrap();
    fs.fail(Op::Fsync, 1, Failure::Io);
    assert!(log.delete_before(None).is_err());
    assert_eq!(log.start_offset(), offset(0), "the start did not move");
    assert!(
        fs.contents(&dir.join("log-start")).is_none(),
        "and nothing was recorded"
    );
}

#[test]
fn a_start_that_reached_only_the_page_cache_is_made_durable_before_segments_are_removed_on_its_account()
 {
    // DeleteRecords renamed its record into place and the directory flush failed: the start is in the page cache
    // and nowhere else. A restarted process reads it and removes the segments below it. If a power cut then
    // took the start back while keeping some of those removals, the log would have a hole; so the removals are
    // preceded by recording the start again, durably.
    for seed in 0..48 {
        let (fs, dir, mut log) = three_segments(0);
        log.sync().unwrap();
        fs.fail(Op::DirSync, 1, Failure::Io);
        assert!(log.delete_before(Some(offset(5))).is_err());
        drop(log);
        let fs = fs.process_restart();
        let reopened = open(&fs, &dir, 2 * one(), Trust::Unvouched);
        assert_eq!(reopened.start_offset(), offset(5));
        drop(reopened);
        let after = fs.power_cut(Cut {
            data: DataCut::Drop,
            names: NameCut::Random,
            seed,
        });
        let log = open(&after, &dir, 2 * one(), Trust::Unvouched);
        assert!(
            log.start_offset() >= offset(5),
            "seed {seed}: the start went back to {}",
            log.start_offset()
        );
        let mut next = log.start_offset();
        let mut seen = Vec::new();
        while next < log.end_offset() {
            let got = log.reader().read(next, usize::MAX, None).unwrap();
            for b in kafka_wire::batch::Batches::new(got) {
                let b = b.unwrap();
                seen.push(b.base_offset().unwrap().get());
                next = b
                    .base_offset()
                    .unwrap()
                    .checked_add(b.record_count())
                    .unwrap();
            }
        }
        assert!(
            seen.windows(2).all(|w| w[1] == w[0] + 1),
            "seed {seed}: a hole in {seen:?}"
        );
    }
}
