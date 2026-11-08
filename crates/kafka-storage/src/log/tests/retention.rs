use super::*;
use crate::retention::count_to_drop;
use crate::sim::{Cut, DataCut, NameCut};
use crate::testutil::offset;
use kafka_wire::batch::TimestampType;
use kafka_wire::ids::TimestampMs;

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
        log.append(
            &ones(1, -1),
            TimestampMs::from_wire(1_000),
            TimestampType::Create,
        )
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

/// The decision for sealed segments of the given sizes and ages, and how many ages it had to work out.
fn decide(sizes: &[u64], ages: &[i64], active: u64, policy: Retention, now: i64) -> (usize, usize) {
    let looked_at = std::cell::Cell::new(0);
    let age_of = |i: usize| {
        looked_at.set(looked_at.get() + 1);
        Ok(ages[i])
    };
    let n = count_to_drop(sizes, age_of, active, policy, TimestampMs::from_wire(now)).unwrap();
    (n, looked_at.get())
}

#[test]
fn the_decision_is_a_pure_function_of_sizes_and_ages() {
    let day = 86_400_000;
    let (sizes, ages) = ([100, 100, 100], [day, 2 * day, 3 * day]);
    let now = 3 * day + 1;
    let by = |max_bytes, max_age_ms| Retention {
        max_bytes,
        max_age_ms,
    };
    let go = |policy| decide(&sizes, &ages, 50, policy, now).0;
    assert_eq!(go(by(None, None)), 0);
    assert_eq!(
        go(by(None, Some(day))),
        2,
        "older than a day: the first two"
    );
    assert_eq!(go(by(Some(200), None)), 2, "350 held, 200 allowed: two go");
    assert_eq!(go(by(Some(1_000), None)), 0);
    assert_eq!(go(by(Some(0), None)), 3, "never more than the sealed ones");
    // Age first, then size on what is left: 250 held after the first goes, 100 allowed, so all three go.
    assert_eq!(go(by(Some(100), Some(day / 2))), 3);
    assert_eq!(decide(&[], &[], 50, by(Some(0), Some(0)), now).0, 0);
    // The boundary: a segment exactly as old as the cutoff is kept, one a millisecond older is not.
    assert_eq!(decide(&[1], &[now - day], 0, by(None, Some(day)), now).0, 0);
    assert_eq!(
        decide(&[1], &[now - day - 1], 0, by(None, Some(day)), now).0,
        1
    );
}

#[test]
fn an_age_is_only_worked_out_when_it_is_needed() {
    let (sizes, ages) = ([1; 5], [0, 0, 1_000, 0, 0]); // the third is the first that is not too old
    let by = |max_bytes, max_age_ms| Retention {
        max_bytes,
        max_age_ms,
    };
    assert_eq!(
        decide(&sizes, &ages, 0, by(Some(3), None), 500),
        (2, 0),
        "size alone never reads a segment"
    );
    assert_eq!(
        decide(&sizes, &ages, 0, by(None, Some(100)), 500),
        (2, 3),
        "up to the first that is not expired"
    );
    assert_eq!(decide(&sizes, &ages, 0, by(None, None), 500), (0, 0));
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

#[test]
fn trimming_a_partition_that_is_not_open_records_the_start_and_makes_the_removals_durable() {
    let (fs, dir, mut log) = three_segments(0);
    log.sync().unwrap();
    drop(log);
    let policy = Retention {
        max_bytes: Some(2 * one()),
        max_age_ms: None,
    };
    assert_eq!(crate::enforce_closed(&fs, &dir, policy, NOW).unwrap(), 2);
    let after = fs.power_cut(Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    });
    assert_eq!(
        names(&after, &dir),
        vec!["00000000000000000004.log"],
        "the removals reached the disk"
    );
    assert_eq!(
        after.contents(&dir.join("log-start")).as_deref(),
        Some(&b"4\n"[..]),
        "and so did the start"
    );
}
