use super::*;
use crate::error::StorageError;
use crate::sim::{Failure, Op, SimFs};
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
