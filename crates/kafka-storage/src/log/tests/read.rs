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
fn an_index_that_points_past_the_log_or_a_log_cut_short_is_damage_and_not_a_shorter_answer() {
    // Both would otherwise read as "nothing in this segment" and carry on in the next, skipping its records.
    let build = || {
        let (fs, dir, mut log) = fresh(3 * one());
        for _ in 0..9 {
            log.append(&ones(1, 0), NOW, TimestampType::Create).unwrap();
        }
        (fs, dir, log)
    };
    let (fs, dir, log) = build();
    let index = fs
        .open_existing(&dir.join("00000000000000000003.index"))
        .unwrap();
    let past = [4i64.to_be_bytes(), 100_000i64.to_be_bytes()].concat();
    crate::fs::FsFile::write_all_at(&index, &past, 0).unwrap();
    // Offset 4 is looked up in the index. (Offset 5 began the segment's last append, so it is found from that hint with no
    // lookup, and read correctly: the index is not consulted to be found damaged.)
    assert!(log.reader().read(offset(4), usize::MAX, None).is_err());

    let (fs, dir, log) = build();
    let file = fs
        .open_existing(&dir.join("00000000000000000003.log"))
        .unwrap();
    crate::fs::FsFile::set_len(&file, 100).unwrap(); // cut under the open log: the headers promise more
    assert!(log.reader().read(offset(3), usize::MAX, None).is_err());
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

#[test]
fn a_read_with_a_limit_just_ahead_does_not_ask_the_file_for_what_max_bytes_would_allow() {
    let (_fs, _dir, mut log) = fresh(NEVER);
    // Some twenty megabytes in one segment, a batch of 2,000 records at a time.
    for _ in 0..1_000 {
        log.append(&[batch(2_000, 0)], NOW, TimestampType::Create)
            .unwrap();
    }
    let r = log.reader();
    let whole = r.read(offset(0), usize::MAX, None).unwrap();
    assert!(whole.len() > 4 << 20, "{} bytes in all", whole.len());
    // A read-committed fetch whose last stable offset is a few batches in, with a fifty-megabyte allowance.
    super::super::read::BIGGEST_ASK.with(|b| b.set(0));
    let got = r
        .read(offset(0), 50 << 20, Some(offset(2_000 * 5)))
        .unwrap();
    assert_eq!(
        bases(&got),
        [0, 2_000, 4_000, 6_000, 8_000],
        "up to the limit"
    );
    let asked = super::super::read::BIGGEST_ASK.with(std::cell::Cell::get);
    assert!(
        asked < whole.len() / 10,
        "asked the file for {asked} bytes of {}",
        whole.len()
    );
    // And the whole log is still read whole when nothing limits it.
    assert_eq!(r.read(offset(0), usize::MAX, None).unwrap(), whole);
}

#[test]
fn a_small_answer_cut_from_a_large_read_is_copied_and_a_large_one_is_not() {
    let (_fs, _dir, mut log) = fresh(NEVER);
    for _ in 0..400 {
        log.append(&[batch(50, 0)], NOW, TimestampType::Create)
            .unwrap();
    }
    let r = log.reader();
    super::super::read::COPIED.with(|c| c.set(0));
    // One batch of a 50 MiB allowance read from a log that has plenty: the file is asked for a good deal more than it
    // returns (the headroom and the batches to step over), and the answer is not left as a window on that.
    let small = r.read(offset(500), 1, None).unwrap();
    assert_eq!(bases(&small), [500]);
    assert_eq!(super::super::read::COPIED.with(std::cell::Cell::get), 1);
    // Most of what was read is what is returned: kept as it is.
    let big = r.read(offset(0), 1 << 20, None).unwrap();
    assert!(big.len() > 100 << 10);
    assert_eq!(super::super::read::COPIED.with(std::cell::Cell::get), 1);
}

/// A reader that keeps up asks for the offset where the last append (or its own last read) ended: it is found with no
/// index lookup, so the read is one read of the file. An offset in the middle still goes through the index, and the
/// answers are the same either way.
#[test]
fn a_reader_that_keeps_up_reads_once_with_no_index_lookup() {
    let (fs, _dir, mut log) = fresh(NEVER);
    // Enough batches for many index entries (one per 150 bytes), so a lookup takes several reads.
    for _ in 0..64 {
        log.append(&[batch(3, 0)], NOW, TimestampType::Create)
            .unwrap();
    }
    let r = log.reader();
    // Caught up at the end, and woken by an append: the read begins where the append did.
    let end = log.end_offset();
    log.append(&[batch(3, 0)], NOW, TimestampType::Create)
        .unwrap();
    let unread = r.unread_near_the_end(end);
    assert_eq!(
        unread,
        Some(r.read(end, usize::MAX, None).unwrap().len() as u64)
    );

    // Fallen behind by two appends: the read begins where its own last read ended.
    log.append(&[batch(3, 0)], NOW, TimestampType::Create)
        .unwrap();
    log.append(&[batch(3, 0)], NOW, TimestampType::Create)
        .unwrap();
    let next = offset(end.get() + 3);
    let before = fs.count(Op::Read);
    let tail = r.read(next, usize::MAX, None).unwrap();
    assert_eq!(bases(&tail), vec![next.get(), next.get() + 3]);
    assert_eq!(
        fs.count(Op::Read) - before,
        1,
        "the data, and nothing of the index"
    );

    // The reader's own last read is a hint too: a second append, read from where the first read ended.
    log.append(&[batch(3, 0)], NOW, TimestampType::Create)
        .unwrap();
    log.append(&[batch(3, 0)], NOW, TimestampType::Create)
        .unwrap();
    let from = offset(next.get() + 6);
    assert_eq!(bases(&r.read(from, 1, None).unwrap()), vec![from.get()]);
    let before = fs.count(Op::Read);
    assert_eq!(
        bases(&r.read(offset(from.get() + 3), usize::MAX, None).unwrap()),
        vec![from.get() + 3]
    );
    assert_eq!(fs.count(Op::Read) - before, 1);

    // An offset in the middle has no hint, and is looked up.
    assert_eq!(r.unread_near_the_end(offset(30)), None);
    let before = fs.count(Op::Read);
    assert_eq!(bases(&r.read(offset(30), 1, None).unwrap()), vec![30]);
    assert!(fs.count(Op::Read) - before > 1, "an index lookup");
}

/// A hint that does not hold a batch where it says is not trusted: the read looks the offset up and answers as it would
/// have with no hint.
#[test]
fn a_hint_that_does_not_point_at_a_batch_falls_back_to_the_index() {
    let (_fs, _dir, mut log) = fresh(NEVER);
    for _ in 0..8 {
        log.append(&[batch(3, 0)], NOW, TimestampType::Create)
            .unwrap();
    }
    let r = log.reader();
    let segments = r.shared.segments.load();
    let active = segments.last().unwrap();
    // Offset 12 said to be at a position inside a batch.
    active.remember(12, 7);
    assert_eq!(
        bases(&r.read(offset(12), usize::MAX, None).unwrap()),
        vec![12, 15, 18, 21]
    );
}
