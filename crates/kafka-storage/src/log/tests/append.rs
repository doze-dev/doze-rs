use super::*;
use crate::error::StorageError;
use crate::fs::Fs;
use crate::sim::{Failure, Op};
use crate::testutil::{batch, offset};
use kafka_wire::batch::TimestampType;
use kafka_wire::ids::{Offset, TimestampMs};

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

#[test]
fn running_out_of_offsets_takes_back_what_was_written() {
    let (fs, dir) = crate::testutil::disk();
    let base = i64::MAX - 5;
    for ext in ["log", "index"] {
        fs.open_or_create(&dir.join(format!("{base:020}.{ext}")))
            .unwrap();
    }
    let mut log = open(&fs, &dir, NEVER, Trust::Unvouched);
    assert!(
        log.append(&[batch(10, 0)], NOW, TimestampType::Create)
            .is_err()
    );
    assert_eq!(
        fs.contents(&dir.join(format!("{base:020}.log")))
            .unwrap()
            .len(),
        0,
        "the bytes were cut off"
    );
    // The next append lands where the failed one began, and reads back as itself.
    assert_eq!(
        log.append(&[batch(5, 0)], NOW, TimestampType::Create)
            .unwrap()
            .next_offset,
        offset(i64::MAX)
    );
    assert_eq!(
        bases(&log.reader().read(offset(base), usize::MAX, None).unwrap()),
        vec![base]
    );
}

#[test]
fn a_batch_bigger_than_a_segment_goes_in_an_empty_segment_without_a_roll() {
    let (fs, dir, mut log) = fresh(10);
    log.append(&ones(3, 0), NOW, TimestampType::Create).unwrap();
    assert_eq!(
        fs.names(&dir).len(),
        2,
        "one segment, not two opened over each other: {:?}",
        fs.names(&dir)
    );
    log.append(&ones(1, 0), NOW, TimestampType::Create).unwrap();
    assert_eq!(
        fs.names(&dir).len(),
        4,
        "now it is full, and the next append rolls"
    );
}

#[test]
fn a_failed_segment_start_leaves_the_directory_to_be_flushed_by_the_next_sync() {
    let (fs, _dir, mut log) = fresh(2 * one());
    log.append(&ones(2, 0), NOW, TimestampType::Create).unwrap();
    log.sync().unwrap();
    fs.fail(Op::Create, 2, Failure::Io); // the new segment's log is made; its index is not
    assert!(log.append(&ones(1, 0), NOW, TimestampType::Create).is_err());
    assert!(
        !log.is_offline(),
        "a segment that cannot be started is not a failed flush"
    );
    let before = fs.count(Op::DirSync);
    log.sync().unwrap();
    assert!(
        fs.count(Op::DirSync) > before,
        "the create and the removal were never flushed"
    );
}
