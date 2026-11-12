use super::super::tests::{NEVER, NOW, fresh, one, ones};
use super::*;
use crate::sim::{Failure, Op};
use kafka_wire::batch::TimestampType;

#[test]
fn a_log_of_several_segments_lists_each_with_its_base_and_size() {
    // Segments that hold two one-record batches each.
    let (_fs, _dir, mut log) = fresh(one() * 2);
    for _ in 0..5 {
        log.append(&ones(1, 0), NOW, TimestampType::Create).unwrap();
    }
    let segments = log.reader().segments();
    let bases: Vec<i64> = segments.iter().map(|s| s.base.get()).collect();
    assert_eq!(bases, vec![0, 2, 4], "oldest first, named for their first record");
    let sizes: Vec<u64> = segments.iter().map(|s| s.bytes).collect();
    assert_eq!(sizes, vec![one() * 2, one() * 2, one()]);
    assert!(segments.iter().all(|s| s.modified_ms.is_some()));
}

#[test]
fn a_new_log_has_one_empty_segment_and_is_not_offline() {
    let (_fs, _dir, log) = fresh(NEVER);
    let reader = log.reader();
    assert_eq!(reader.segments().len(), 1);
    assert_eq!(reader.segments()[0].bytes, 0);
    assert_eq!(reader.offline_cause(), None);
}

#[test]
fn what_the_directory_says_is_what_the_open_log_says() {
    let (fs, dir, mut log) = fresh(one() * 2);
    for _ in 0..5 {
        log.append(&ones(1, 0), NOW, TimestampType::Create).unwrap();
    }
    log.sync().unwrap();
    let open = log.reader().segments();
    let closed = segments_on_disk(&fs, &dir).unwrap();
    assert_eq!(open.len(), closed.len());
    for (a, b) in open.iter().zip(&closed) {
        assert_eq!((a.base, a.bytes), (b.base, b.bytes));
    }
}

#[test]
fn a_directory_that_is_not_there_has_no_segments_and_foreign_files_are_not_segments() {
    let (fs, dir, _log) = fresh(NEVER);
    assert!(
        segments_on_disk(&fs, &dir.join("nowhere"))
            .unwrap()
            .is_empty()
    );
    fs.open_or_create(&dir.join("5.log")).unwrap();
    fs.open_or_create(&dir.join("notes.txt")).unwrap();
    let found = segments_on_disk(&fs, &dir).unwrap();
    assert_eq!(found.len(), 1, "only the log's own segment: {found:?}");
}

#[test]
fn a_failed_flush_is_the_offline_cause_a_reader_can_read() {
    let (fs, _dir, mut log) = fresh(NEVER);
    log.append(&ones(1, 0), NOW, TimestampType::Create).unwrap();
    let reader = log.reader();
    assert_eq!(reader.offline_cause(), None);
    fs.fail(Op::Fsync, 1, Failure::Io);
    assert!(log.sync().is_err());
    let cause = reader.offline_cause().expect("offline after the failed flush");
    assert!(!cause.is_empty());
    assert!(log.is_offline());
    // Reads, and the facts about files, carry on.
    assert_eq!(reader.segments().len(), 1);
}
