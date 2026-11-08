use super::*;
use crate::fs::{Fs, FsFile};
use crate::sim::{Cut, DataCut, NameCut, SimFs};
use crate::testutil::offset;
use kafka_wire::batch::TimestampType;

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
fn a_recorded_start_beyond_the_records_that_survived_is_carried_on_from_never_reissued() {
    // DeleteRecords is recorded durably; the records it covered may not have been. Offsets below the recorded
    // start must never be handed out again, so the log carries on from it.
    let (fs, dir) = written(3, NEVER);
    crate::logstart::write(&fs, &dir, offset(50)).unwrap();
    let mut log = reopen(&fs, &dir);
    assert_eq!(
        (log.start_offset(), log.end_offset()),
        (offset(50), offset(50))
    );
    assert!(log.reader().read(offset(50), 100, None).unwrap().is_empty());
    assert_eq!(
        log.append(&ones(1, 0), NOW, TimestampType::Create)
            .unwrap()
            .first_offset,
        offset(50)
    );
    // And it stays so after another restart and a power cut that loses the new segment's unflushed bytes.
    log.sync().unwrap();
    drop(log);
    let after = fs.power_cut(Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    });
    let log = reopen(&after, &dir);
    assert_eq!(
        (log.start_offset(), log.end_offset()),
        (offset(50), offset(51))
    );
    let logs: Vec<_> = after
        .names(&dir)
        .into_iter()
        .filter(|n| n.ends_with(".log"))
        .collect();
    assert_eq!(
        logs,
        vec!["00000000000000000050.log"],
        "the segment below the start is gone"
    );
}

#[test]
fn a_vouched_open_walks_only_the_end_of_the_log_and_an_unvouched_one_checks_all_of_it() {
    // The last stop flushed a vouched log whole, so only the stretch after the last index entry is looked at (with
    // its checksums). A damaged batch before it is not found; that is the point, and what vouching means. Without
    // the vouching every batch is checked and the log ends at the damage.
    let damage = |fs: &SimFs, dir: &std::path::Path| {
        let file = fs.open_existing(&dir.join(LOG0)).unwrap();
        file.write_all_at(&[0xff; 4], one() + 40).unwrap(); // the body of the second batch
    };
    let (fs, dir) = written(6, NEVER);
    damage(&fs, &dir);
    assert_eq!(
        open(&fs, &dir, NEVER, Trust::Unvouched).end_offset(),
        offset(1)
    );
    let (fs, dir) = written(6, NEVER);
    damage(&fs, &dir);
    assert_eq!(
        open(&fs, &dir, NEVER, Trust::Vouched).end_offset(),
        offset(6)
    );
}

#[test]
fn a_vouched_open_does_not_believe_an_index_entry_that_points_at_the_end_of_the_log() {
    // Found by the recovery fuzzer: the walk from such an entry has nothing to walk, and found it whole, so the
    // log claimed an end offset it had no records for.
    let (fs, dir) = written(3, NEVER);
    let size = fs.contents(&dir.join(LOG0)).unwrap().len() as i64;
    let index = fs.open_existing(&dir.join(INDEX0)).unwrap();
    let bogus = [9_000i64.to_be_bytes(), size.to_be_bytes()].concat();
    index.write_all_at(&bogus, 0).unwrap();
    let log = open(&fs, &dir, NEVER, Trust::Vouched);
    assert_eq!(
        log.end_offset(),
        offset(3),
        "the log is what it holds, not what its index says"
    );
    assert_eq!(bases(&all(&log.reader())), vec![0, 1, 2]);
}
