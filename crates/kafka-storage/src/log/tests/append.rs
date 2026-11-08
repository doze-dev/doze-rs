use super::*;
use crate::error::StorageError;
use crate::fs::Fs;
use crate::sim::{Failure, Op};
use crate::testutil::{batch, offset};
use kafka_wire::batch::TimestampType;
use kafka_wire::ids::{Offset, TimestampMs};

#[test]
fn a_new_partition_is_empty_with_one_segment() {
    let (fs, dir, log) = fresh(NEVER);
    assert_eq!(
        (log.start_offset(), log.end_offset()),
        (offset(0), offset(0))
    );
    assert_eq!(
        fs.names(&dir),
        vec!["00000000000000000000.index", "00000000000000000000.log"]
    );
    let reader = log.reader();
    assert!(
        reader.read(offset(0), 100, None).unwrap().is_empty(),
        "caught up"
    );
}

#[test]
fn offsets_are_assigned_at_append_and_stamped_into_the_batches() {
    let (_fs, _dir, mut log) = fresh(NEVER);
    let first = log
        .append(&[batch(2, 10), batch(3, 20)], NOW, TimestampType::Create)
        .unwrap();
    assert_eq!(
        (first.first_offset, first.next_offset),
        (offset(0), offset(5))
    );
    let second = log
        .append(&[batch(1, 30)], NOW, TimestampType::Create)
        .unwrap();
    assert_eq!(
        (second.first_offset, second.next_offset),
        (offset(5), offset(6))
    );
    assert_eq!(log.end_offset(), offset(6));
    assert_eq!(bases(&all(&log.reader())), vec![0, 2, 5]);
}

#[test]
fn a_read_starts_at_the_batch_that_holds_the_offset() {
    let (_fs, _dir, mut log) = fresh(NEVER);
    log.append(
        &[batch(3, 0), batch(3, 0), batch(3, 0)],
        NOW,
        TimestampType::Create,
    )
    .unwrap();
    let r = log.reader();
    for (from, want) in [
        (0, vec![0, 3, 6]),
        (2, vec![0, 3, 6]),
        (3, vec![3, 6]),
        (5, vec![3, 6]),
        (8, vec![6]),
    ] {
        assert_eq!(
            bases(&r.read(offset(from), usize::MAX, None).unwrap()),
            want,
            "from {from}"
        );
    }
    assert!(
        r.read(offset(9), 100, None).unwrap().is_empty(),
        "at the end: caught up"
    );
    let err = r.read(offset(10), 100, None).unwrap_err();
    assert!(matches!(
        err,
        StorageError::OutOfRange {
            requested: 10,
            start: 0,
            end: 9
        }
    ));
}

#[test]
fn a_read_holds_whole_batches_within_the_byte_limit_but_always_the_first() {
    let (_fs, _dir, mut log) = fresh(NEVER);
    log.append(&ones(5, 0), NOW, TimestampType::Create).unwrap();
    let r = log.reader();
    let n = one() as usize;
    assert_eq!(bases(&r.read(offset(0), 2 * n, None).unwrap()), vec![0, 1]);
    assert_eq!(
        bases(&r.read(offset(0), 2 * n + n - 1, None).unwrap()),
        vec![0, 1],
        "no half a batch"
    );
    assert_eq!(
        bases(&r.read(offset(2), 1, None).unwrap()),
        vec![2],
        "the first, however large"
    );
}

#[test]
fn a_limit_hides_the_batches_that_begin_at_or_after_it() {
    let (_fs, _dir, mut log) = fresh(NEVER);
    log.append(&ones(5, 0), NOW, TimestampType::Create).unwrap();
    let r = log.reader();
    assert_eq!(
        bases(&r.read(offset(0), usize::MAX, Some(offset(3))).unwrap()),
        vec![0, 1, 2]
    );
    assert!(
        r.read(offset(3), usize::MAX, Some(offset(3)))
            .unwrap()
            .is_empty(),
        "the first is past the limit"
    );
    assert_eq!(
        bases(&r.read(offset(0), usize::MAX, Some(offset(99))).unwrap()),
        vec![0, 1, 2, 3, 4]
    );
}

#[test]
fn a_reader_sees_only_appended_payloads_and_a_clone_sees_the_same() {
    let (_fs, _dir, mut log) = fresh(NEVER);
    let (a, b) = (log.reader(), log.reader().clone());
    log.append(&ones(2, 0), NOW, TimestampType::Create).unwrap();
    assert_eq!(bases(&all(&a)), vec![0, 1]);
    assert_eq!(bases(&all(&b)), vec![0, 1]);
    assert_eq!((a.end_offset(), b.start_offset()), (offset(2), offset(0)));
}

#[test]
fn a_reader_can_wait_for_the_end_to_move() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let (_fs, _dir, mut log) = fresh(NEVER);
    let mut end = log.reader().watch_end();
    rt.block_on(async {
        // Subscribed first, appended after: the change is not missed.
        log.append(&ones(1, 0), NOW, TimestampType::Create).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), end.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(*end.borrow_and_update(), offset(1));
    });
}

#[test]
fn a_payload_that_does_not_fit_rolls_first_and_goes_in_whole() {
    let (fs, dir, mut log) = fresh(3 * one());
    log.append(&ones(2, 0), NOW, TimestampType::Create).unwrap();
    // 2 + 2 batches would pass the segment size: the roll comes first and both go in the new segment.
    log.append(&ones(2, 0), NOW, TimestampType::Create).unwrap();
    assert_eq!(
        fs.names(&dir).len(),
        4,
        "two segments: {:?}",
        fs.names(&dir)
    );
    let second = fs.contents(&dir.join("00000000000000000002.log")).unwrap();
    assert_eq!(second.len() as u64, 2 * one());
    assert_eq!(
        bases(&all(&log.reader())),
        vec![0, 1, 2, 3],
        "reads carry across the segments"
    );
    // A first payload bigger than a segment is not rolled away from: an empty segment takes what it is given.
    let (_fs, _dir, mut tiny) = fresh(10);
    tiny.append(&ones(3, 0), NOW, TimestampType::Create)
        .unwrap();
    assert_eq!(tiny.end_offset(), offset(3));
}

#[test]
fn a_roll_flushes_the_segment_it_seals_before_the_next_exists() {
    let (fs, _dir, mut log) = fresh(2 * one());
    log.append(&ones(2, 0), NOW, TimestampType::Create).unwrap();
    log.append(&ones(1, 0), NOW, TimestampType::Create).unwrap();
    let trace = fs.trace();
    let at = |op: Op, name: &str| {
        trace
            .iter()
            .position(|(o, p)| *o == op && p.to_string_lossy().ends_with(name))
    };
    let flushed = at(Op::Fsync, "0000.log").or_else(|| at(Op::Fsync, "00000000000000000000.log"));
    let created = at(Op::Create, "00000000000000000002.log");
    assert!(flushed.unwrap() < created.unwrap(), "{trace:?}");
}

#[test]
fn a_payload_is_appended_whole_or_not_at_all() {
    let (fs, _dir, mut log) = fresh(NEVER);
    log.append(&ones(1, 0), NOW, TimestampType::Create).unwrap();
    // Writes of the next payload: batch (1), its index entry? no (the interval is 150): batch (2) fails.
    fs.fail(Op::Write, 2, Failure::Io);
    let err = log
        .append(&ones(3, 0), NOW, TimestampType::Create)
        .unwrap_err();
    assert!(matches!(err, StorageError::Io { .. }));
    assert!(!log.is_offline(), "a failed write is not a failed flush");
    assert_eq!(log.end_offset(), offset(1), "no offset was used");
    assert_eq!(
        bases(&all(&log.reader())),
        vec![0],
        "none of the payload is visible"
    );
    let again = log.append(&ones(3, 0), NOW, TimestampType::Create).unwrap();
    assert_eq!(
        again.first_offset,
        offset(1),
        "the retry gets the offsets the failed try would have had"
    );
    assert_eq!(bases(&all(&log.reader())), vec![0, 1, 2, 3]);
}

#[test]
fn a_full_disk_refuses_writes_and_they_resume_when_there_is_room() {
    let (fs, _dir, mut log) = fresh(NEVER);
    log.append(&ones(1, 0), NOW, TimestampType::Create).unwrap();
    fs.set_capacity(Some(one() + 16 + 30));
    assert!(log.append(&ones(2, 0), NOW, TimestampType::Create).is_err());
    assert_eq!(log.end_offset(), offset(1));
    fs.set_capacity(None);
    assert_eq!(
        log.append(&ones(2, 0), NOW, TimestampType::Create)
            .unwrap()
            .next_offset,
        offset(3)
    );
    assert_eq!(bases(&all(&log.reader())), vec![0, 1, 2]);
}

#[test]
fn an_empty_payload_changes_nothing() {
    let (_fs, _dir, mut log) = fresh(NEVER);
    let a = log.append(&[], NOW, TimestampType::Create).unwrap();
    assert_eq!((a.first_offset, a.next_offset), (offset(0), offset(0)));
}

#[test]
fn offsets_run_out_without_wrapping() {
    // A partition whose only segment begins just below the top of the offset space.
    let (fs, dir) = crate::testutil::disk();
    let base = i64::MAX - 5;
    let name = format!("{base:020}");
    for ext in ["log", "index"] {
        fs.open_or_create(&dir.join(format!("{name}.{ext}")))
            .unwrap();
    }
    let mut log = open(&fs, &dir, NEVER, Trust::Unvouched);
    assert_eq!(log.end_offset(), offset(base));
    let err = log
        .append(&[batch(10, 0)], NOW, TimestampType::Create)
        .unwrap_err();
    assert!(matches!(err, StorageError::Io { .. }));
    assert_eq!(log.end_offset(), offset(base), "nothing was taken");
    assert!(
        log.reader()
            .read(offset(base), 100, None)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        log.append(&[batch(5, 0)], NOW, TimestampType::Create)
            .unwrap()
            .next_offset,
        offset(i64::MAX)
    );
}

#[test]
fn a_log_append_time_topic_stamps_the_brokers_clock_and_keeps_the_batches_valid() {
    let (_fs, _dir, mut log) = fresh(NEVER);
    let producer_time = 5;
    let sent = ones(3, producer_time);
    let at = TimestampMs::from_wire(777);
    log.append(&sent, at, TimestampType::LogAppend).unwrap();
    let read = all(&log.reader());
    for stored in kafka_wire::batch::Batches::new(read) {
        let stored = stored.expect("the checksum was recomputed for the new header");
        assert_eq!(
            stored.attributes().timestamp_type(),
            TimestampType::LogAppend
        );
        assert_eq!(stored.max_timestamp(), at);
    }
    // The caller's batches are untouched: the log wrote a stamped header and the body as it was.
    assert!(
        sent.iter()
            .all(|b| b.max_timestamp().get() == producer_time
                && b.base_offset() == Some(Offset::ZERO))
    );
}
