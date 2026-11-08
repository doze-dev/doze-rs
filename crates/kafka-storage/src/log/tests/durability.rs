use super::*;
use crate::error::StorageError;
use crate::fs::Fs;
use crate::sim::{Cut, DataCut, Failure, NameCut, Op, SimFs};
use crate::testutil::offset;

#[test]
fn a_sync_with_nothing_dirty_makes_no_system_call() {
    let (fs, _dir, mut log) = fresh(NEVER);
    log.append(&ones(1, 0), NOW, TimestampType::Create).unwrap();
    log.sync().unwrap();
    let before = fs.ops();
    log.sync().unwrap();
    log.sync().unwrap();
    assert_eq!(fs.ops(), before);
}

#[test]
fn what_is_read_back_at_open_is_flushed_by_the_first_sync_unless_the_last_stop_vouches() {
    for (trust, flushes) in [(Trust::Unvouched, true), (Trust::Vouched, false)] {
        let (fs, dir, mut log) = fresh(NEVER);
        log.append(&ones(2, 0), NOW, TimestampType::Create).unwrap();
        log.sync().unwrap();
        drop(log);
        let fs = fs.process_restart();
        let mut log = open(&fs, &dir, NEVER, trust);
        let ticket = log.ticket();
        assert_eq!(ticket.is_durable(), !flushes, "{trust:?}");
        let before = fsyncs(&fs);
        log.sync().unwrap();
        assert_eq!(
            fsyncs(&fs) > before,
            flushes,
            "{trust:?}: a flush of what a killed process left in the cache"
        );
        assert!(ticket.is_durable());
    }
}

#[test]
fn a_failed_fsync_takes_the_partition_offline_and_is_never_retried() {
    let (fs, _dir, mut log) = fresh(NEVER);
    let ticket = log
        .append(&ones(1, 0), NOW, TimestampType::Create)
        .unwrap()
        .ticket;
    fs.fail(Op::Fsync, 1, Failure::Io);
    let err = log.sync().unwrap_err();
    assert!(matches!(err, StorageError::Io { .. }));
    assert!(log.is_offline());
    let attempted = fsyncs(&fs);
    // Everything that writes is refused, with the first failure named, and nothing tries the disk again.
    assert!(matches!(
        log.sync().unwrap_err(),
        StorageError::Offline { .. }
    ));
    assert!(matches!(
        log.append(&ones(1, 0), NOW, TimestampType::Create)
            .unwrap_err(),
        StorageError::Offline { .. }
    ));
    assert!(matches!(
        log.delete_before(Some(offset(1))).unwrap_err(),
        StorageError::Offline { .. }
    ));
    assert_eq!(fsyncs(&fs), attempted);
    // The ticket of the append that was not made durable learns it.
    let outcome = block_on(ticket.durable());
    assert!(
        matches!(outcome, Err(StorageError::Offline { .. })),
        "{outcome:?}"
    );
    // Reads carry on.
    assert_eq!(bases(&all(&log.reader())), vec![0]);
}

#[test]
fn a_failed_directory_flush_takes_the_partition_offline_too() {
    let (fs, _dir, mut log) = fresh(NEVER);
    log.append(&ones(1, 0), NOW, TimestampType::Create).unwrap();
    fs.fail(Op::DirSync, 1, Failure::Io);
    assert!(log.sync().is_err());
    assert!(log.is_offline());
}

#[test]
fn a_retry_that_would_have_succeeded_proves_nothing_and_so_is_not_made() {
    // The kernel's worst case: after a failed flush the pages are dropped from the account and the next flush
    // of the file succeeds. A log that retried would acknowledge records that a power cut then takes.
    let (fs, dir, mut log) = fresh(NEVER);
    fs.lose_pages_on_failed_fsync(true);
    log.append(&ones(1, 0), NOW, TimestampType::Create).unwrap();
    log.sync().unwrap();
    let doomed = log
        .append(&ones(1, 0), NOW, TimestampType::Create)
        .unwrap()
        .ticket;
    fs.fail(Op::Fsync, 1, Failure::Io);
    assert!(log.sync().is_err());
    assert!(
        log.sync().is_err(),
        "refused, not retried into a false success"
    );
    assert!(!doomed.is_durable());
    let after = fs.power_cut(Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    });
    let log = open(&after, &dir, NEVER, Trust::Unvouched);
    assert_eq!(log.end_offset(), offset(1), "only what had been flushed");
}

#[test]
fn a_power_cut_keeps_what_was_synced_and_may_lose_the_rest() {
    let (fs, dir, mut log) = fresh(NEVER);
    log.append(&ones(2, 0), NOW, TimestampType::Create).unwrap();
    log.sync().unwrap();
    log.append(&ones(3, 0), NOW, TimestampType::Create).unwrap(); // acknowledged to nobody
    for (data, names) in [
        (DataCut::Drop, NameCut::Durable),
        (DataCut::Zero, NameCut::Durable),
        (DataCut::Keep, NameCut::Volatile),
    ] {
        let after = fs.power_cut(Cut {
            data,
            names,
            seed: 3,
        });
        let reopened = open(&after, &dir, NEVER, Trust::Unvouched);
        assert!(
            reopened.end_offset() >= offset(2),
            "{data:?}: the synced records survive"
        );
        let reader = reopened.reader();
        assert_eq!(bases(&all(&reader))[..2], [0, 1], "{data:?}");
    }
}

#[test]
fn a_new_segments_name_reaches_the_disk_with_the_sync_that_acknowledges_something_in_it() {
    let (fs, dir, mut log) = fresh(2 * one());
    log.append(&ones(2, 0), NOW, TimestampType::Create).unwrap();
    log.sync().unwrap();
    log.append(&ones(1, 0), NOW, TimestampType::Create).unwrap(); // rolls: the new segment's name is only in memory
    let cut = |fs: &SimFs| {
        fs.power_cut(Cut {
            data: DataCut::Drop,
            names: NameCut::Durable,
            seed: 1,
        })
    };
    assert!(
        !cut(&fs)
            .names(&dir)
            .contains(&"00000000000000000002.log".to_string())
    );
    log.sync().unwrap();
    assert!(
        cut(&fs)
            .names(&dir)
            .contains(&"00000000000000000002.log".to_string())
    );
}

#[test]
fn close_flushes_everything() {
    let (fs, dir, mut log) = fresh(NEVER);
    log.append(&ones(4, 0), NOW, TimestampType::Create).unwrap();
    log.close().unwrap();
    let after = fs.power_cut(Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    });
    assert_eq!(
        open(&after, &dir, NEVER, Trust::Unvouched).end_offset(),
        offset(4)
    );
    let _ = fs
        .open_existing(&dir.join("00000000000000000000.log"))
        .unwrap();
}

fn fsynced(fs: &SimFs, suffix: &str) -> bool {
    fs.trace()
        .iter()
        .any(|(op, p)| *op == Op::Fsync && p.to_string_lossy().ends_with(suffix))
}

#[test]
fn what_is_read_back_at_open_is_flushed_log_index_and_directory_each_by_itself() {
    // The two files and the directory are three separate claims: a process that restarts before it flushed
    // the index it rewrote must still flush it, or a power cut after the next roll leaves a stale sealed index.
    let (fs, dir, mut log) = fresh(NEVER);
    log.append(&ones(2, 0), NOW, TimestampType::Create).unwrap();
    log.sync().unwrap();
    drop(log);
    let fs = fs.process_restart();
    let mut log = open(&fs, &dir, NEVER, Trust::Unvouched);
    let before = fs.ops();
    log.sync().unwrap();
    let trace = fs.trace();
    let flushed: Vec<_> = trace
        .iter()
        .skip(before as usize)
        .map(|(op, p)| (*op, p.file_name().unwrap().to_string_lossy().into_owned()))
        .collect();
    assert!(
        flushed.contains(&(Op::Fsync, "00000000000000000000.log".into())),
        "{flushed:?}"
    );
    assert!(
        flushed.contains(&(Op::Fsync, "00000000000000000000.index".into())),
        "{flushed:?}"
    );
    assert!(
        flushed.contains(&(Op::DirSync, "t-0".into())),
        "{flushed:?}"
    );
    // A vouched open owes none of it.
    let fs2 = fs.process_restart();
    let mut vouched = open(&fs2, &dir, NEVER, Trust::Vouched);
    let before = fs2.ops();
    vouched.sync().unwrap();
    assert_eq!(fs2.ops(), before);
    assert!(!fsynced(&fs2, ".index"));
}

#[test]
fn a_sync_after_a_failed_write_still_flushes_the_cut_that_took_it_back() {
    let (fs, _dir, mut log) = fresh(NEVER);
    log.append(&ones(1, 0), NOW, TimestampType::Create).unwrap();
    log.sync().unwrap();
    fs.fail(Op::Write, 1, Failure::Io);
    assert!(log.append(&ones(1, 0), NOW, TimestampType::Create).is_err());
    // Nothing was acknowledged since the last sync, but the taken-back bytes must reach the disk too.
    let before = fs.ops();
    log.sync().unwrap();
    assert!(fs.ops() > before, "the cut was never flushed");
}
