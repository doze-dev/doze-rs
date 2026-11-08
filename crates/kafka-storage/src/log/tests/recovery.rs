use super::*;
use crate::fs::{Fs, FsFile};
use crate::sim::{Cut, DataCut, NameCut, SimFs};
use crate::testutil::{offset, raw};
use kafka_wire::batch::TimestampType;

const LOG0: &str = "00000000000000000000.log";
const INDEX0: &str = "00000000000000000000.index";

/// A log with `n` one-record batches, flushed, then "killed": the disk the next process starts from.
fn written(n: usize, segment_bytes: u64) -> (SimFs, std::path::PathBuf) {
    let (fs, dir, mut log) = fresh(segment_bytes);
    for _ in 0..n {
        log.append(&ones(1, 5), NOW, TimestampType::Create).unwrap();
    }
    log.sync().unwrap();
    drop(log);
    (fs.process_restart(), dir)
}

fn reopen(fs: &SimFs, dir: &std::path::Path) -> PartitionLog<SimFs> {
    open(fs, dir, NEVER, Trust::Unvouched)
}

#[test]
fn reopening_restores_the_log_byte_for_byte_and_appends_continue() {
    let (fs, dir) = written(5, NEVER);
    let before = fs.contents(&dir.join(LOG0)).unwrap();
    let mut log = reopen(&fs, &dir);
    assert_eq!(
        (log.start_offset(), log.end_offset()),
        (offset(0), offset(5))
    );
    assert_eq!(all(&log.reader()), before);
    assert_eq!(
        log.append(&ones(1, 0), NOW, TimestampType::Create)
            .unwrap()
            .first_offset,
        offset(5)
    );
    assert_eq!(bases(&all(&log.reader())), vec![0, 1, 2, 3, 4, 5]);
}

#[test]
fn a_torn_tail_is_cut_and_the_cut_is_flushed() {
    let (fs, dir) = written(3, NEVER);
    let whole = fs.contents(&dir.join(LOG0)).unwrap().len() as u64;
    let file = fs.open_existing(&dir.join(LOG0)).unwrap();
    file.write_all_at(&raw(3, 1, 0)[..40], whole).unwrap(); // an append that never finished
    let log = reopen(&fs, &dir);
    assert_eq!(log.end_offset(), offset(3));
    assert_eq!(
        fs.contents(&dir.join(LOG0)).unwrap().len() as u64,
        whole,
        "the torn bytes are gone"
    );
    // A second crash right after does not bring them back.
    let again = fs.power_cut(Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    });
    assert_eq!(again.contents(&dir.join(LOG0)).unwrap().len() as u64, whole);
}

#[test]
fn a_zero_filled_tail_after_a_power_cut_is_cut() {
    let (fs, dir, mut log) = fresh(NEVER);
    log.append(&ones(2, 0), NOW, TimestampType::Create).unwrap();
    log.sync().unwrap();
    log.append(&ones(3, 0), NOW, TimestampType::Create).unwrap();
    let after = fs.power_cut(Cut {
        data: DataCut::Zero,
        names: NameCut::Durable,
        seed: 1,
    });
    assert!(
        after.contents(&dir.join(LOG0)).unwrap().len() > (2 * one()) as usize,
        "the length survived, the data did not"
    );
    let log = open(&after, &dir, NEVER, Trust::Unvouched);
    assert_eq!(log.end_offset(), offset(2));
}

#[test]
fn a_damaged_batch_in_the_middle_ends_the_log_there() {
    let (fs, dir) = written(4, NEVER);
    let file = fs.open_existing(&dir.join(LOG0)).unwrap();
    let middle = 2 * one() + 40;
    file.write_all_at(&[0xff; 4], middle).unwrap();
    let log = reopen(&fs, &dir);
    assert_eq!(
        log.end_offset(),
        offset(2),
        "every batch is checked when nothing vouches for the files"
    );
}

#[test]
fn an_index_that_disagrees_with_the_log_is_rebuilt() {
    let (fs, dir) = written(6, NEVER);
    let want = fs.contents(&dir.join(INDEX0)).unwrap();
    let index = fs.open_existing(&dir.join(INDEX0)).unwrap();
    index.write_all_at(&[7; 16], want.len() as u64).unwrap(); // an entry for bytes that are not there
    index.write_all_at(&[1], want.len() as u64 + 16).unwrap(); // and half of another
    let log = reopen(&fs, &dir);
    assert_eq!(fs.contents(&dir.join(INDEX0)).unwrap(), want);
    assert_eq!(
        bases(&log.reader().read(offset(4), usize::MAX, None).unwrap()),
        vec![4, 5]
    );
    // A missing index file is rebuilt the same way.
    let (fs, dir) = written(6, NEVER);
    fs.remove_file(&dir.join(INDEX0)).unwrap();
    let log = reopen(&fs, &dir);
    assert_eq!(fs.contents(&dir.join(INDEX0)).unwrap(), want);
    assert_eq!(
        bases(&log.reader().read(offset(5), usize::MAX, None).unwrap()),
        vec![5]
    );
}

#[test]
fn the_index_of_a_sealed_segment_is_rebuilt_when_it_is_gone() {
    let (fs, dir) = written(6, 2 * one());
    let sealed_index = "00000000000000000000.index";
    let want = fs.contents(&dir.join(sealed_index)).unwrap();
    fs.remove_file(&dir.join(sealed_index)).unwrap();
    let log = reopen(&fs, &dir);
    assert_eq!(fs.contents(&dir.join(sealed_index)).unwrap(), want);
    assert_eq!(bases(&all(&log.reader())), vec![0, 1, 2, 3, 4, 5]);
}

#[test]
fn leftovers_of_interrupted_operations_are_removed() {
    let (fs, dir) = written(2, NEVER);
    fs.open_or_create(&dir.join("log-start.tmp")).unwrap();
    fs.open_or_create(&dir.join("00000000000000000099.index"))
        .unwrap(); // an index with no log
    let _log = reopen(&fs, &dir);
    assert_eq!(fs.names(&dir), vec![INDEX0, LOG0]);
}

#[test]
fn a_roll_that_died_after_creating_the_next_segment_leaves_an_empty_active_segment() {
    let (fs, dir) = written(2, NEVER);
    fs.open_or_create(&dir.join("00000000000000000002.log"))
        .unwrap();
    fs.open_or_create(&dir.join("00000000000000000002.index"))
        .unwrap();
    let mut log = reopen(&fs, &dir);
    assert_eq!(log.end_offset(), offset(2));
    assert_eq!(
        log.append(&ones(1, 0), NOW, TimestampType::Create)
            .unwrap()
            .first_offset,
        offset(2)
    );
    assert_eq!(bases(&all(&log.reader())), vec![0, 1, 2]);
}

#[test]
fn segments_below_the_recorded_start_are_removed_at_open() {
    let (fs, dir) = written(6, 2 * one()); // three segments: 0, 2, 4
    crate::logstart::write(&fs, &dir, offset(3)).unwrap(); // the crash came after this, before the removals
    let log = reopen(&fs, &dir);
    assert_eq!(log.start_offset(), offset(3));
    let names = fs.names(&dir);
    assert!(
        !names.iter().any(|n| n.starts_with("00000000000000000000")),
        "{names:?}"
    );
    assert!(
        names.iter().any(|n| n.starts_with("00000000000000000002")),
        "the segment holding the start stays"
    );
    assert_eq!(bases(&all(&log.reader())), vec![3, 4, 5]);
}

#[test]
fn a_recorded_start_past_the_end_is_not_believed() {
    let (fs, dir) = written(3, NEVER);
    crate::logstart::write(&fs, &dir, offset(50)).unwrap();
    let log = reopen(&fs, &dir);
    assert_eq!(log.start_offset(), offset(3), "capped at the end");
}

#[test]
fn an_empty_segment_a_failed_roll_left_behind_cannot_hide_the_records_that_followed_it() {
    // The simulation found this: a roll created the next segment's log, could not create its index, and removed
    // the log again, and none of that was flushed. Appends went on in the old segment. A power cut then brought
    // the empty log back, and the reopened log believed it ended where the leftover began.
    let (fs, dir) = written(3, NEVER);
    let mut log = reopen(&fs, &dir);
    log.append(&ones(2, 5), NOW, TimestampType::Create).unwrap(); // records 3 and 4 live in segment 0
    log.sync().unwrap();
    drop(log);
    // The leftover: an empty segment that begins inside the one before it.
    fs.open_or_create(&dir.join("00000000000000000004.log"))
        .unwrap();
    fs.open_or_create(&dir.join("00000000000000000004.index"))
        .unwrap();
    let log = reopen(&fs, &dir);
    assert_eq!(
        log.end_offset(),
        offset(5),
        "the records after the leftover's base are not hidden"
    );
    assert_eq!(bases(&all(&log.reader())), vec![0, 1, 2, 3, 4]);
    assert_eq!(
        fs.names(&dir),
        vec![INDEX0, LOG0],
        "and the leftover is removed"
    );
}

#[test]
fn a_segment_that_starts_exactly_where_the_one_before_ends_is_not_a_leftover() {
    let (fs, dir) = written(4, 2 * one());
    let log = reopen(&fs, &dir);
    assert_eq!(log.end_offset(), offset(4));
    assert_eq!(
        fs.names(&dir).len(),
        4,
        "two segments, each with its index: {:?}",
        fs.names(&dir)
    );
}
