use super::*;
use crate::error::StorageError;
use crate::retention::{Candidate, count_to_drop};
use crate::sim::SimFs;
use crate::testutil::offset;

fn seg(size: u64, age_ms: i64) -> Candidate {
    Candidate { size, age_ms }
}

fn names(fs: &SimFs, dir: &std::path::Path) -> Vec<String> {
    fs.names(dir)
        .into_iter()
        .filter(|n| n.ends_with(".log"))
        .collect()
}

/// Six one-record batches in segments of two: 0, 2, 4 (the last active).
fn three_segments(ts: i64) -> (SimFs, std::path::PathBuf, PartitionLog<SimFs>) {
    let (fs, dir, mut log) = fresh(2 * one());
    for _ in 0..6 {
        log.append(ones(1, ts), NOW).unwrap();
    }
    (fs, dir, log)
}

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
    log.append(ones(4, 0), NOW).unwrap();
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
    assert_eq!(log.append(ones(1, 0), NOW).unwrap().first_offset, offset(4));
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
fn retention_by_size_deletes_the_oldest_whole_segments_and_never_the_active_one() {
    let (fs, dir, mut log) = three_segments(0);
    let policy = Retention {
        max_bytes: Some(4 * one()),
        max_age_ms: None,
    };
    assert_eq!(
        log.enforce_retention(policy, NOW).unwrap(),
        1,
        "6 batches held, room for 4: the oldest segment goes"
    );
    assert_eq!(log.start_offset(), offset(2));
    assert_eq!(names(&fs, &dir).len(), 2);
    let tight = Retention {
        max_bytes: Some(1),
        max_age_ms: None,
    };
    assert_eq!(
        log.enforce_retention(tight, NOW).unwrap(),
        1,
        "one sealed segment is left to go"
    );
    assert_eq!(
        names(&fs, &dir).len(),
        1,
        "the active segment is never deleted"
    );
    assert_eq!(log.enforce_retention(tight, NOW).unwrap(), 0);
    assert_eq!(bases(&all(&log.reader())), vec![4, 5]);
}

#[test]
fn retention_by_age_goes_by_the_newest_record_in_a_segment_not_by_when_it_was_written() {
    // Records stamped long ago (a replay, a backfill) are old at the next sweep however new the file is.
    let (fs, _dir, mut log) = three_segments(NOW.get() - 10_000);
    fs.set_now_ms(NOW.get()); // every file was written just now
    let policy = Retention {
        max_bytes: None,
        max_age_ms: Some(5_000),
    };
    assert_eq!(log.enforce_retention(policy, NOW).unwrap(), 2);
    assert_eq!(log.start_offset(), offset(4));
}

#[test]
fn a_segment_with_no_timestamps_is_as_old_as_its_last_write() {
    let (fs, dir) = crate::testutil::disk();
    fs.set_now_ms(1_000);
    let mut log = open(&fs, &dir, 2 * one(), Trust::Unvouched);
    for _ in 0..4 {
        log.append(ones(1, -1), TimestampMs::from_wire(1_000))
            .unwrap();
    }
    let policy = Retention {
        max_bytes: None,
        max_age_ms: Some(500),
    };
    let late = TimestampMs::from_wire(2_000);
    assert_eq!(log.enforce_retention(policy, late).unwrap(), 1);
    let early = TimestampMs::from_wire(1_200);
    assert_eq!(log.enforce_retention(policy, early).unwrap(), 0);
}

#[test]
fn no_bounds_deletes_nothing() {
    let (_fs, _dir, mut log) = three_segments(0);
    assert_eq!(log.enforce_retention(Retention::default(), NOW).unwrap(), 0);
}

#[test]
fn the_decision_is_a_pure_function_of_sizes_and_ages() {
    let day = 86_400_000;
    let sealed = [seg(100, day), seg(100, 2 * day), seg(100, 3 * day)];
    let now = TimestampMs::from_wire(3 * day + 1);
    let by = |max_bytes, max_age_ms| Retention {
        max_bytes,
        max_age_ms,
    };
    assert_eq!(count_to_drop(&sealed, 50, by(None, None), now), 0);
    assert_eq!(
        count_to_drop(&sealed, 50, by(None, Some(day)), now),
        2,
        "older than a day: the first two"
    );
    assert_eq!(
        count_to_drop(&sealed, 50, by(Some(200), None), now),
        2,
        "350 held, 200 allowed: two go"
    );
    assert_eq!(count_to_drop(&sealed, 50, by(Some(1_000), None), now), 0);
    assert_eq!(
        count_to_drop(&sealed, 50, by(Some(0), None), now),
        3,
        "never more than the sealed ones"
    );
    // Age first, then size on what is left: 250 held after the first goes, 100 allowed, so all three go.
    assert_eq!(
        count_to_drop(&sealed, 50, by(Some(100), Some(day / 2)), now),
        3
    );
    assert_eq!(count_to_drop(&[], 50, by(Some(0), Some(0)), now), 0);
}

#[test]
fn a_partition_that_is_not_open_is_trimmed_the_same_way() {
    let (fs, dir, mut log) = three_segments(0);
    log.sync().unwrap();
    let open_result = {
        let (fs2, dir2, mut log2) = three_segments(0);
        log2.sync().unwrap();
        drop(log2);
        let r = crate::enforce_closed(
            &fs2,
            &dir2,
            Retention {
                max_bytes: Some(4 * one()),
                max_age_ms: None,
            },
            NOW,
        )
        .unwrap();
        (r, names(&fs2, &dir2))
    };
    drop(log);
    let removed = crate::enforce_closed(
        &fs,
        &dir,
        Retention {
            max_bytes: Some(4 * one()),
            max_age_ms: None,
        },
        NOW,
    )
    .unwrap();
    assert_eq!((removed, names(&fs, &dir)), open_result);
    assert_eq!(removed, 1);
    // The start follows from the first segment left, the next time it is opened.
    let log = open(&fs, &dir, 2 * one(), Trust::Unvouched);
    assert_eq!(log.start_offset(), offset(2));
}

use crate::sim::{Cut, DataCut, NameCut};
