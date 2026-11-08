use super::*;
use crate::fs::{Fs, FsFile};
use crate::scan::scan;
use crate::sim::{Failure, Op, SimFs};
use crate::testutil::{batch, disk, offset, raw};

const INTERVAL: u64 = 4096;

fn active(fs: &SimFs, dir: &std::path::Path, base: i64, interval: u64) -> ActiveSegment<SimFs> {
    ActiveSegment::open(fs, dir, offset(base), interval)
        .unwrap()
        .0
}

fn one() -> u64 {
    raw(0, 1, 0).len() as u64
}

#[test]
fn names_are_twenty_digits_and_round_trip() {
    let dir = std::path::Path::new("/d");
    assert_eq!(
        log_path(dir, offset(7)).to_str(),
        Some("/d/00000000000000000007.log")
    );
    assert_eq!(
        index_path(dir, offset(7)).to_str(),
        Some("/d/00000000000000000007.index")
    );
    assert_eq!(base_of_log("00000000000000000042.log"), Some(offset(42)));
    assert_eq!(
        base_of_index("00000000000000000042.index"),
        Some(offset(42))
    );
    assert_eq!(base_of_log("42.index"), None);
    assert_eq!(base_of_log("log-start"), None);
    assert_eq!(base_of_log("-5.log"), None, "an offset is not negative");
    assert_eq!(base_of_log("abc.log"), None);
}

#[test]
fn a_new_segment_says_it_created_its_files_and_a_reopened_one_does_not() {
    let (fs, dir) = disk();
    let (_, made) = ActiveSegment::open(&fs, &dir, offset(0), INTERVAL).unwrap();
    assert!(made);
    let (_, made) = ActiveSegment::open(&fs, &dir, offset(0), INTERVAL).unwrap();
    assert!(!made);
}

#[test]
fn nothing_is_visible_to_a_reader_until_it_is_published() {
    let (fs, dir) = disk();
    let mut seg = active(&fs, &dir, 0, INTERVAL);
    let view = seg.view();
    seg.append(&batch(1, 5).stamped(offset(0), None), 100)
        .unwrap();
    assert_eq!((view.committed(), seg.size()), (0, one()));
    assert!(view.read(0, 1000).unwrap().is_empty());
    seg.publish();
    assert_eq!(view.committed(), one());
    assert_eq!(view.read(0, 10_000).unwrap().len() as u64, one());
    // A read is clamped to what is committed, and none starts past it.
    assert_eq!(view.read(10, 5).unwrap().len(), 5);
    assert_eq!(view.read(one() - 2, 100).unwrap().len(), 2);
    assert!(view.read(one(), 100).unwrap().is_empty());
}

#[test]
fn the_index_gets_the_first_batch_and_then_one_per_interval() {
    let (fs, dir) = disk();
    let mut seg = active(&fs, &dir, 10, 2 * one());
    for i in 0..5 {
        seg.append(&batch(1, 0).stamped(offset(10 + i), None), 0)
            .unwrap();
    }
    seg.publish();
    let at = |offset, batches| crate::index::Entry {
        offset,
        position: batches * one(),
    };
    let index = seg.view().index().clone();
    assert!(index.matches(&[at(10, 0), at(12, 2), at(14, 4)]).unwrap());
    assert!(!index.matches(&[at(10, 0), at(12, 2)]).unwrap(), "fewer");
    assert!(
        !index.matches(&[at(10, 0), at(12, 2), at(14, 5)]).unwrap(),
        "different"
    );
    let view = seg.view();
    assert_eq!(
        view.seek(9).unwrap(),
        0,
        "below every entry: from the start"
    );
    assert_eq!(view.seek(11).unwrap(), 0);
    assert_eq!(view.seek(12).unwrap(), 2 * one());
    assert_eq!(view.seek(13).unwrap(), 2 * one());
    assert_eq!(view.seek(1_000).unwrap(), 4 * one());
}

#[test]
fn a_failed_write_leaves_no_trace() {
    let (fs, dir) = disk();
    let mut seg = active(&fs, &dir, 0, INTERVAL);
    seg.append(&batch(1, 7).stamped(offset(0), None), 1)
        .unwrap();
    seg.publish();
    let mark = (seg.size(), seg.view().index().len());
    fs.fail(Op::Write, 1, Failure::Io);
    assert!(
        seg.append(&batch(1, 9).stamped(offset(1), None), 2)
            .is_err()
    );
    assert_eq!((seg.size(), seg.view().index().len()), mark);
    // The next append goes where the failed one would have, once.
    seg.append(&batch(1, 9).stamped(offset(1), None), 3)
        .unwrap();
    seg.publish();
    let scanned = scan(
        seg.view().log_file().as_ref(),
        seg.size(),
        offset(0),
        INTERVAL,
        0,
    )
    .unwrap();
    assert_eq!(
        (scanned.valid_end, scanned.next_offset),
        (seg.size(), offset(2))
    );
}

#[test]
fn a_write_cut_short_by_a_full_disk_is_taken_back() {
    let (fs, dir) = disk();
    let mut seg = active(&fs, &dir, 0, INTERVAL);
    seg.append(&batch(1, 0).stamped(offset(0), None), 1)
        .unwrap();
    seg.publish();
    fs.set_capacity(Some(one() + 16 + 20)); // the first batch, its index entry, and part of the next
    let err = seg
        .append(&batch(1, 0).stamped(offset(1), None), 2)
        .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::StorageFull);
    fs.set_capacity(None);
    assert_eq!(
        fs.contents(&log_path(&dir, offset(0))).unwrap().len() as u64,
        one(),
        "the partial bytes were cut off"
    );
    seg.append(&batch(1, 0).stamped(offset(1), None), 3)
        .unwrap();
}

#[test]
fn a_failed_index_write_takes_back_the_batch_it_belonged_to() {
    let (fs, dir) = disk();
    let mut seg = active(&fs, &dir, 0, INTERVAL);
    fs.fail(Op::Write, 2, Failure::Io); // the batch is the first write; the first index entry is the second
    assert!(
        seg.append(&batch(1, 0).stamped(offset(0), None), 1)
            .is_err()
    );
    assert_eq!(seg.size(), 0);
    assert_eq!(fs.contents(&log_path(&dir, offset(0))).unwrap().len(), 0);
    seg.append(&batch(1, 0).stamped(offset(0), None), 2)
        .unwrap();
    assert_eq!(seg.view().index().len(), 0, "not published, but written");
    seg.publish();
    assert_eq!(seg.view().index().len(), 1);
}

#[test]
fn a_mark_takes_back_a_run_of_appends() {
    let (fs, dir) = disk();
    let mut seg = active(&fs, &dir, 0, one());
    seg.append(&batch(1, 5).stamped(offset(0), None), 1)
        .unwrap();
    let mark = seg.mark();
    seg.append(&batch(1, 50).stamped(offset(1), None), 2)
        .unwrap();
    seg.append(&batch(1, 500).stamped(offset(2), None), 3)
        .unwrap();
    seg.rewind(mark);
    seg.publish();
    assert_eq!(seg.size(), one());
    assert_eq!(seg.view().index().len(), 1);
    assert_eq!(
        fs.contents(&log_path(&dir, offset(0))).unwrap().len() as u64,
        one()
    );
    let sealed = seg.into_sealed();
    assert_eq!(
        sealed.max_timestamp().unwrap().map(|t| t.get()),
        Some(5),
        "the taken-back timestamps are forgotten"
    );
}

#[test]
fn sync_flushes_only_what_changed() {
    let (fs, dir) = disk();
    let mut seg = active(&fs, &dir, 0, INTERVAL);
    let fsyncs = |fs: &SimFs| fs.trace().iter().filter(|(op, _)| *op == Op::Fsync).count();
    seg.sync().unwrap();
    assert_eq!(fsyncs(&fs), 0, "nothing written: no system call");
    seg.append(&batch(1, 0).stamped(offset(0), None), 1)
        .unwrap();
    assert!(seg.is_dirty());
    seg.sync().unwrap();
    assert_eq!(
        fsyncs(&fs),
        2,
        "the log and the index it gained an entry in"
    );
    seg.append(&batch(1, 0).stamped(offset(1), None), 1)
        .unwrap(); // no new index entry
    seg.sync().unwrap();
    assert_eq!(fsyncs(&fs), 3, "only the log");
    seg.sync().unwrap();
    assert_eq!(fsyncs(&fs), 3);
    assert!(!seg.is_dirty());
}

#[test]
fn a_sealed_segment_keeps_what_the_active_one_knew() {
    let (fs, dir) = disk();
    let mut seg = active(&fs, &dir, 0, INTERVAL);
    seg.append(&batch(1, 20).stamped(offset(0), None), 1_000)
        .unwrap();
    seg.append(&batch(1, -1).stamped(offset(1), None), 2_000)
        .unwrap(); // -1: no timestamp
    seg.append(&batch(1, 30).stamped(offset(2), None), 3_000)
        .unwrap();
    seg.publish();
    seg.sync().unwrap();
    let sealed = seg.into_sealed();
    assert_eq!(
        (sealed.base(), sealed.size(), sealed.modified_ms()),
        (offset(0), 3 * one(), 3_000)
    );
    assert_eq!(sealed.max_timestamp().unwrap().map(|t| t.get()), Some(30));
    assert_eq!(
        sealed.view().read(0, 10_000).unwrap().len() as u64,
        3 * one()
    );
}

#[test]
fn a_sealed_segment_read_back_finds_its_newest_timestamp_when_asked() {
    let (fs, dir) = disk();
    let mut seg = active(&fs, &dir, 0, INTERVAL);
    seg.append(&batch(1, 20).stamped(offset(0), None), 1)
        .unwrap();
    seg.append(&batch(1, 40).stamped(offset(1), None), 2)
        .unwrap();
    seg.publish();
    seg.sync().unwrap();
    drop(seg);
    let sealed = SealedSegment::open(&fs, &dir, offset(0), INTERVAL).unwrap();
    assert_eq!(sealed.size(), 2 * one());
    assert_eq!(sealed.max_timestamp().unwrap().map(|t| t.get()), Some(40));
    assert!(SealedSegment::open(&fs, &dir, offset(99), INTERVAL).is_err());
}

#[test]
fn adopting_a_scan_cuts_the_log_durably_and_rebuilds_the_index() {
    let (fs, dir) = disk();
    let mut seg = active(&fs, &dir, 0, one());
    for i in 0..3 {
        seg.append(&batch(1, 0).stamped(offset(i), None), 1)
            .unwrap();
    }
    seg.publish();
    seg.sync().unwrap();
    fs.sync_dir(&dir).unwrap();
    // A torn fourth batch, and an index that went stale.
    let log = fs.open_existing(&log_path(&dir, offset(0))).unwrap();
    log.write_all_at(&raw(3, 1, 0)[..30], 3 * one()).unwrap();
    log.sync_data().unwrap();
    drop(seg);

    let (mut seg, _) = ActiveSegment::open(&fs, &dir, offset(0), one()).unwrap();
    let size = seg.view().log_file().size().unwrap();
    let scanned = scan(seg.view().log_file().as_ref(), size, offset(0), one(), 0).unwrap();
    assert_eq!(scanned.valid_end, 3 * one());
    seg.adopt(&scanned).unwrap();
    assert_eq!((seg.size(), seg.view().committed()), (3 * one(), 3 * one()));
    // Nothing since was flushed by us except the cut itself; a power cut keeps the cut.
    let after = fs.power_cut(crate::sim::Cut {
        data: crate::sim::DataCut::Drop,
        names: crate::sim::NameCut::Durable,
        seed: 1,
    });
    assert_eq!(
        after.contents(&log_path(&dir, offset(0))).unwrap().len() as u64,
        3 * one()
    );
}
