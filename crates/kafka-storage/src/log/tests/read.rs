use super::*;
use crate::error::StorageError;
use crate::fs::Fs;
use crate::sim::Op;
use crate::testutil::offset;
use kafka_wire::batch::TimestampType;

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
fn damage_in_committed_bytes_is_an_error_and_never_a_gap_to_read_past() {
    // Sealed segments are not re-checked, so a header that rots on the disk must be reported when it is read.
    let (fs, dir, mut log) = fresh(3 * one());
    for _ in 0..9 {
        log.append(&ones(1, 0), NOW, TimestampType::Create).unwrap();
    }
    let sealed = dir.join("00000000000000000003.log");
    let file = fs.open_existing(&sealed).unwrap();
    crate::fs::FsFile::write_all_at(&file, &[0xff; 4], 16).unwrap(); // the magic of the segment's first batch
    let reader = log.reader();
    assert_eq!(
        bases(&reader.read(offset(0), usize::MAX, None).unwrap()),
        vec![0, 1, 2]
    );
    let err = reader.read(offset(3), usize::MAX, None).unwrap_err();
    assert!(
        matches!(&err, StorageError::Io { source, .. } if source.kind() == std::io::ErrorKind::InvalidData),
        "{err:?}"
    );
    assert!(
        reader.read(offset(4), usize::MAX, None).is_err(),
        "and not skipped past either"
    );
}

#[test]
fn a_read_of_many_small_batches_asks_the_file_for_a_few_ranges_not_one_per_batch() {
    let (fs, _dir, mut log) = fresh(NEVER);
    for _ in 0..40 {
        log.append(&ones(50, 0), NOW, TimestampType::Create)
            .unwrap(); // 2000 batches of one record
    }
    let reader = log.reader();
    let before = fs.count(Op::Read);
    let got = reader.read(offset(500), 64 << 10, None).unwrap();
    let reads = fs.count(Op::Read) - before;
    assert!(
        got.len() > 60 << 10 && got.len() <= 64 << 10,
        "{} bytes",
        got.len()
    );
    assert!(
        reads <= 20, // a binary search of the index (log2 of its entries) and a range or two of the log
        "{reads} reads for {} batches",
        bases(&got).len()
    );
}
