use super::*;
use crate::retention::count_to_drop;
use crate::sim::{Cut, DataCut, NameCut};
use crate::testutil::offset;
use kafka_wire::batch::TimestampType;
use kafka_wire::ids::TimestampMs;

#[test]
fn retention_by_size_deletes_the_oldest_whole_segments_while_the_log_keeps_the_bound_without_them()
{
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
        "4 held, 1 allowed: the sealed segment goes"
    );
    assert_eq!(names(&fs, &dir).len(), 1);
    assert_eq!(
        log.enforce_retention(tight, NOW).unwrap(),
        0,
        "the active segment stays: the log would hold less than the bound without it"
    );
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
    assert_eq!(
        log.enforce_retention(policy, NOW).unwrap(),
        3,
        "all three are old, the active one too"
    );
    assert_eq!(log.start_offset(), offset(6));
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
    assert_eq!(log.enforce_retention(policy, late).unwrap(), 2);
    let early = TimestampMs::from_wire(1_200);
    assert_eq!(log.enforce_retention(policy, early).unwrap(), 0);
}

/// The decision for segments of the given sizes and ages (the last is the active one), and how many ages it had to work out.
fn decide(sizes: &[u64], ages: &[i64], policy: Retention, now: i64) -> (usize, usize) {
    let looked_at = std::cell::Cell::new(0);
    let age_of = |i: usize| {
        looked_at.set(looked_at.get() + 1);
        Ok(ages[i])
    };
    let n = count_to_drop(sizes, age_of, policy, TimestampMs::from_wire(now)).unwrap();
    (n, looked_at.get())
}

#[test]
fn the_decision_is_a_pure_function_of_sizes_and_ages() {
    let day = 86_400_000;
    let (sizes, ages) = ([100, 100, 100, 50], [day, 2 * day, 3 * day, 3 * day + 1]);
    let now = 3 * day + 1;
    let by = |max_bytes, max_age_ms| Retention {
        max_bytes,
        max_age_ms,
    };
    let go = |policy| decide(&sizes, &ages, policy, now).0;
    assert_eq!(go(by(None, None)), 0);
    assert_eq!(
        go(by(None, Some(day))),
        2,
        "older than a day: the first two"
    );
    assert_eq!(
        go(by(Some(200), None)),
        1,
        "350 held, 200 allowed: one goes, and 250 are left; a second would leave 150"
    );
    assert_eq!(go(by(Some(250), None)), 1, "exactly the bound is left");
    assert_eq!(go(by(Some(251), None)), 0, "one byte short of it is not");
    assert_eq!(go(by(Some(1_000), None)), 0);
    assert_eq!(
        go(by(Some(0), None)),
        4,
        "a bound of nothing takes the active segment too"
    );
    // Age first, then size on what is left: 150 held after two go; 100 allowed, and a third would leave 50.
    assert_eq!(go(by(Some(100), Some(day / 2))), 2);
    assert_eq!(decide(&[], &[], by(Some(0), Some(0)), now).0, 0);
    // The boundary: a segment exactly as old as the cutoff is kept, one a millisecond older is not.
    assert_eq!(decide(&[1], &[now - day], by(None, Some(day)), now).0, 0);
    assert_eq!(
        decide(&[1], &[now - day - 1], by(None, Some(day)), now).0,
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
        decide(&sizes, &ages, by(Some(3), None), 500),
        (2, 0),
        "size alone never reads a segment"
    );
    assert_eq!(
        decide(&sizes, &ages, by(None, Some(100)), 500),
        (2, 3),
        "up to the first that is not expired"
    );
    assert_eq!(decide(&sizes, &ages, by(None, None), 500), (0, 0));
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

fn by_age(max_age_ms: i64) -> Retention {
    Retention {
        max_bytes: None,
        max_age_ms: Some(max_age_ms),
    }
}

#[test]
fn an_active_segment_that_has_expired_goes_and_a_fresh_one_takes_its_place() {
    let (fs, dir, mut log) = fresh(NEVER);
    for _ in 0..5 {
        log.append(
            &ones(1, 1_000),
            TimestampMs::from_wire(1_000),
            TimestampType::Create,
        )
        .unwrap();
    }
    let later = TimestampMs::from_wire(10_000);
    assert_eq!(log.enforce_retention(by_age(500), later).unwrap(), 1);
    assert_eq!(log.start_offset(), offset(5), "everything was old");
    assert_eq!(
        log.end_offset(),
        offset(5),
        "and no offset is lost or reused"
    );
    assert_eq!(names(&fs, &dir), vec!["00000000000000000005.log"]);
    assert_eq!(
        log.enforce_retention(by_age(500), later).unwrap(),
        0,
        "an empty active segment is never deleted"
    );
    // It takes appends, where the log left off, and is found whole after a restart.
    log.append(&ones(1, 9_999), later, TimestampType::Create)
        .unwrap();
    log.sync().unwrap();
    drop(log);
    let again = reopen(&fs.process_restart(), &dir);
    assert_eq!(
        (again.start_offset(), again.end_offset()),
        (offset(5), offset(6))
    );
}

#[test]
fn a_bound_of_no_bytes_takes_everything_and_a_young_active_segment_is_kept() {
    let (_fs, _dir, mut log) = three_segments(NOW.get());
    let nothing = Retention {
        max_bytes: Some(0),
        max_age_ms: None,
    };
    assert_eq!(log.enforce_retention(nothing, NOW).unwrap(), 3);
    assert_eq!(log.start_offset(), log.end_offset());
    // Two old segments and a young active one: only the old ones go.
    let (fs, dir, mut log) = fresh(2 * one());
    for _ in 0..4 {
        log.append(&ones(1, 1_000), NOW, TimestampType::Create)
            .unwrap();
    }
    log.append(&ones(1, NOW.get()), NOW, TimestampType::Create)
        .unwrap();
    assert_eq!(names(&fs, &dir).len(), 3);
    assert_eq!(log.enforce_retention(by_age(500), NOW).unwrap(), 2);
    assert_eq!(log.start_offset(), offset(4));
    assert_eq!(names(&fs, &dir), vec!["00000000000000000004.log"]);
}

#[test]
fn a_segment_older_than_segment_ms_is_rolled_by_the_next_append_and_an_idle_one_is_left_alone() {
    let (fs, dir) = disk();
    let mut log = PartitionLog::open(
        fs.clone(),
        dir.clone(),
        LogConfig {
            segment_ms: Some(1_000),
            ..config(NEVER)
        },
        Trust::Unvouched,
    )
    .unwrap();
    let at = |ms| TimestampMs::from_wire(ms);
    log.append(&ones(1, 5), at(10_000), TimestampType::Create)
        .unwrap();
    log.append(&ones(1, 5), at(11_000), TimestampType::Create)
        .unwrap();
    assert_eq!(
        names(&fs, &dir).len(),
        1,
        "a second of age is not more than a second"
    );
    log.append(&ones(1, 5), at(11_001), TimestampType::Create)
        .unwrap();
    assert_eq!(
        names(&fs, &dir).len(),
        2,
        "older than it: the append rolls first"
    );
    assert_eq!(bases(&all(&log.reader())), vec![0, 1, 2]);
    // Nothing is rolled by time alone: with no append, a segment of any age is the same.
    assert_eq!(
        log.enforce_retention(Retention::default(), at(99_999_999))
            .unwrap(),
        0
    );
    assert_eq!(names(&fs, &dir).len(), 2);
}

#[test]
fn a_partition_that_is_not_open_loses_its_expired_active_segment_too_and_starts_a_new_one() {
    let (fs, dir, mut log) = fresh(2 * one());
    for _ in 0..5 {
        log.append(
            &ones(1, 1_000),
            TimestampMs::from_wire(1_000),
            TimestampType::Create,
        )
        .unwrap();
    }
    log.sync().unwrap();
    drop(log);
    let later = TimestampMs::from_wire(10_000);
    assert_eq!(
        crate::enforce_closed(&fs, &dir, by_age(500), later).unwrap(),
        3
    );
    let after = fs.power_cut(Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    });
    assert_eq!(names(&after, &dir), vec!["00000000000000000005.log"]);
    assert_eq!(
        after.contents(&dir.join("log-start")).as_deref(),
        Some(&b"5\n"[..])
    );
    let reopened = reopen(&after, &dir);
    assert_eq!(
        (reopened.start_offset(), reopened.end_offset()),
        (offset(5), offset(5))
    );
    assert_eq!(
        crate::enforce_closed(&after, &dir, by_age(500), later).unwrap(),
        0,
        "nothing is left to remove"
    );
}
