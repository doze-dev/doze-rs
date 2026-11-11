use std::path::Path;
use std::sync::mpsc;

use doze_fs::fs::Fs;
use doze_fs::sim::{Cut, DataCut, Failure, NameCut, Op as FsOp, SimFs};
use redb::{ReadableDatabase, TableDefinition};

use super::*;

const T: TableDefinition<'static, u64, u64> = TableDefinition::new("t");
const DATA: &str = "/data";

fn disk() -> SimFs {
    let fs = SimFs::new();
    fs.create_dir(Path::new(DATA)).unwrap();
    fs.sync_dir(Path::new("/")).unwrap();
    fs
}

fn open(fs: &SimFs) -> Store {
    Store::open(fs, Path::new(DATA), "t.redb", 1 << 20, |txn| {
        txn.open_table(T)?;
        Ok(())
    })
    .unwrap()
}

fn put(key: u64) -> Op {
    Box::new(move |txn| {
        txn.open_table(T)?.insert(key, key * 10)?;
        Ok(())
    })
}

fn get(store: &Store, key: u64) -> Option<u64> {
    let read = store.begin_read().unwrap();
    let table = read.open_table(T).unwrap();
    table.get(key).unwrap().map(|v| v.value())
}

#[test]
fn an_acknowledged_write_is_readable_and_alone_it_costs_one_commit() {
    let fs = disk();
    let store = open(&fs);
    let writer = GroupCommit::start(store.clone()).unwrap();
    writer.submit(put(1)).wait().unwrap();
    assert_eq!(get(&store, 1), Some(10));
    assert_eq!(writer.commits(), 1);
}

#[test]
fn writes_queued_behind_a_slow_one_share_a_commit() {
    let fs = disk();
    let store = open(&fs);
    let writer = GroupCommit::start(store.clone()).unwrap();
    let (release, hold) = mpsc::channel::<()>();
    let (started, running) = mpsc::channel::<()>();
    let slow: Op = Box::new(move |txn| {
        started.send(()).unwrap();
        hold.recv().unwrap();
        txn.open_table(T)?.insert(0, 0)?;
        Ok(())
    });
    let first = writer.submit(slow);
    running.recv().unwrap();
    let rest: Vec<_> = (1..=50).map(|k| writer.submit(put(k))).collect();
    release.send(()).unwrap();
    first.wait().unwrap();
    for ack in rest {
        ack.wait().unwrap();
    }
    // The slow write, then the other fifty together.
    assert_eq!(writer.commits(), 2);
    assert_eq!(get(&store, 50), Some(500));
}

#[test]
fn an_operation_that_fails_before_writing_does_not_hurt_its_neighbours() {
    let fs = disk();
    let store = open(&fs);
    let writer = GroupCommit::start(store.clone()).unwrap();
    let (release, hold) = mpsc::channel::<()>();
    let (started, running) = mpsc::channel::<()>();
    let gate: Op = Box::new(move |_| {
        started.send(()).unwrap();
        hold.recv().unwrap();
        Ok(())
    });
    let gate = writer.submit(gate);
    running.recv().unwrap();
    let a = writer.submit(put(1));
    let bad = writer.submit(Box::new(|_| Err(StoreError::Corrupt("refused".into()))));
    let b = writer.submit(put(2));
    release.send(()).unwrap();
    gate.wait().unwrap();
    a.wait().unwrap();
    assert!(matches!(bad.wait(), Err(StoreError::Corrupt(_))));
    b.wait().unwrap();
    assert_eq!((get(&store, 1), get(&store, 2)), (Some(10), Some(20)));
}

#[test]
fn a_failed_flush_fails_the_batch_and_every_write_after_it() {
    let fs = disk();
    let store = open(&fs);
    let writer = GroupCommit::start(store.clone()).unwrap();
    writer.submit(put(1)).wait().unwrap();
    fs.fail(FsOp::Fsync, 1, Failure::Io);
    assert!(matches!(
        writer.submit(put(2)).wait(),
        Err(StoreError::Failed(_))
    ));
    // The disk is fine again, but the writer does not trust it until the process starts over.
    fs.clear_faults();
    assert!(matches!(
        writer.submit(put(3)).wait(),
        Err(StoreError::Failed(_))
    ));
    assert_eq!(get(&store, 3), None);
}

#[test]
fn a_shut_down_writer_answers_stopped_and_what_it_took_is_kept() {
    let fs = disk();
    let store = open(&fs);
    let writer = GroupCommit::start(store.clone()).unwrap();
    let ack = writer.submit(put(7));
    writer.shutdown();
    ack.wait().unwrap();
    assert_eq!(get(&store, 7), Some(70));
}

#[tokio::test]
async fn an_async_caller_awaits_its_ack() {
    let fs = disk();
    let store = open(&fs);
    let writer = GroupCommit::start(store.clone()).unwrap();
    writer.submit(put(5)).await.unwrap();
    assert_eq!(get(&store, 5), Some(50));
}

/// Everything acknowledged is on the disk after the power goes, in every way it can.
#[test]
fn what_was_acknowledged_survives_every_kind_of_power_cut() {
    let fs = disk();
    let store = open(&fs);
    let writer = GroupCommit::start(store).unwrap();
    for key in 1..=40 {
        writer.submit(put(key)).wait().unwrap();
    }
    for (i, (data, names)) in [
        (DataCut::Drop, NameCut::Durable),
        (DataCut::Zero, NameCut::Durable),
        (DataCut::Prefix, NameCut::Random),
        (DataCut::Pages, NameCut::Random),
        (DataCut::Drop, NameCut::Volatile),
    ]
    .into_iter()
    .enumerate()
    {
        let after = fs.power_cut(Cut {
            data,
            names,
            seed: 17 + i as u64,
        });
        let reopened = open(&after);
        for key in 1..=40 {
            assert_eq!(get(&reopened, key), Some(key * 10), "cut {i}, key {key}");
        }
    }
}
