//! A long read runs on the blocking pool: the one serving thread goes on with other work while it does.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use doze_store::Db;
use doze_store::records::Records;
use redb::{ReadableTable, TableDefinition};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("doze-store-{name}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    dir
}

#[tokio::test(flavor = "current_thread")]
async fn a_read_off_the_loop_leaves_the_serving_thread_free() {
    let dir = scratch("read-off-loop");
    let db = Db::open(&dir, "t.redb", &["rows"]).unwrap();
    let mut rows: Records<u32> = Records::new("rows", "row", 1);
    for i in 0..100 {
        rows.insert(format!("k{i:03}"), i);
    }
    db.submit(vec![rows.stage().unwrap()])
        .unwrap()
        .await
        .unwrap();

    // The read waits for the serving thread to say it ran meanwhile: on the serving thread itself it would wait out the timeout.
    let (ran, seen) = mpsc::channel::<()>();
    let other = tokio::spawn(async move { ran.send(()).unwrap() });
    let counted = db
        .read_off_loop(move |read| {
            let waited = seen.recv_timeout(Duration::from_secs(5)).is_ok();
            let table = read.open_table(TableDefinition::<&str, &[u8]>::new("rows"))?;
            Ok((waited, table.iter()?.count()))
        })
        .await
        .unwrap();
    other.await.unwrap();
    assert_eq!(counted, (true, 100), "the loop ran while the read did");
    db.close();
    drop(std::fs::remove_dir_all(&dir));
}

/// A compaction waits out the queued writes and moves the file's pages: off the serving thread, which goes on meanwhile, and the
/// database is whole and writable after it.
#[tokio::test(flavor = "current_thread")]
async fn a_compaction_off_the_loop_leaves_the_serving_thread_free() {
    let dir = scratch("compact-off-loop");
    let mut db = Db::open(&dir, "t.redb", &["rows"]).unwrap();
    let mut rows: Records<Vec<u8>> = Records::new("rows", "row", 1);
    for i in 0..400 {
        rows.insert(format!("k{i:04}"), vec![7; 16 << 10]);
    }
    db.submit(vec![rows.stage().unwrap()])
        .unwrap()
        .await
        .unwrap();
    for i in 0..390 {
        rows.remove(&format!("k{i:04}"));
    }
    // Queued, not awaited: the compaction waits for it.
    drop(db.submit(vec![rows.stage().unwrap()]));

    let turns = Arc::new(AtomicUsize::new(0));
    let ticker = tokio::spawn({
        let turns = Arc::clone(&turns);
        async move {
            loop {
                turns.fetch_add(1, Ordering::Relaxed);
                tokio::task::yield_now().await;
            }
        }
    });
    let done = db.compact_off_loop().await.unwrap();
    let ran = turns.load(Ordering::Relaxed);
    ticker.abort();
    assert!(
        ran > 0,
        "the serving thread ran other tasks during the compaction"
    );
    assert!(done.freed() > 4 << 20, "{done:?}");
    let left = db.read().unwrap();
    let table = left
        .open_table(TableDefinition::<&str, &[u8]>::new("rows"))
        .unwrap();
    assert_eq!(table.iter().unwrap().count(), 10);
    drop(left);
    rows.insert("again".to_owned(), vec![1]);
    db.submit(vec![rows.stage().unwrap()])
        .unwrap()
        .await
        .unwrap();
    assert!(
        db.get("rows", "again").unwrap().is_some(),
        "the writer is back"
    );
    db.close();
    drop(std::fs::remove_dir_all(&dir));
}
