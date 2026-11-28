//! Read-your-writes: a database's reads see the writes queued for it and not yet committed, in the order they were queued, and the
//! file holds the same once they are, across a reopen.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::mpsc;

use doze_store::records::Records;
use doze_store::rows::RowTable;
use doze_store::{Ack, Db, Op};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("doze-store-{name}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    dir
}

/// Holds the writer until the sender is dropped or sent to: what is submitted meanwhile stays queued.
fn hold(db: &Db) -> (mpsc::Sender<()>, Ack) {
    let (release, wait) = mpsc::channel::<()>();
    let (started, running) = mpsc::channel::<()>();
    let op: Op = Box::new(move |_| {
        started.send(()).unwrap();
        wait.recv().ok();
        Ok(())
    });
    let ack = db.submit_op(op);
    running.recv().unwrap();
    (release, ack)
}

fn text(db: &Db, key: &str) -> Option<String> {
    db.get("recs", key)
        .unwrap()
        .map(|b| doze_store::record::decode::<String>("rec", 1, &b).unwrap())
}

#[test]
fn queued_writes_are_read_in_order_and_the_file_has_the_last_after_a_reopen() {
    let dir = scratch("overlay-reopen");
    let db = Db::open(&dir, "t.redb", &["recs", "rows"]).unwrap();
    let mut recs: Records<String> = Records::new("recs", "rec", 1);
    let mut rows = RowTable::new("rows", false);
    for k in ["a", "b", "c"] {
        rows.put(format!("r/{k}"), k.as_bytes().to_vec());
    }
    db.submit_with_rows(Vec::new(), vec![rows.stage()])
        .unwrap()
        .wait()
        .unwrap();

    let (release, held) = hold(&db);
    let mut acks = Vec::new();
    for v in ["one", "two", "three"] {
        recs.insert("k".into(), v.into());
        acks.push(db.submit(vec![recs.stage().unwrap()]).unwrap());
        assert_eq!(
            text(&db, "k").as_deref(),
            Some(v),
            "the latest queued write is read"
        );
    }
    recs.remove("k");
    recs.insert("j".into(), "jay".into());
    acks.push(db.submit(vec![recs.stage().unwrap()]).unwrap());
    assert_eq!(text(&db, "k"), None, "a queued delete hides the row");
    rows.delete("r/b");
    rows.put("r/d".into(), b"d".to_vec());
    acks.push(db.submit_with_rows(Vec::new(), vec![rows.stage()]).unwrap());
    assert!(!db.pending().is_empty(), "all of it is still queued");

    let keys = |db: &Db, t: &RowTable, reverse| -> Vec<String> {
        t.scan(Some(db), "r/", "r0", 100, reverse)
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect()
    };
    assert_eq!(keys(&db, &rows, false), ["r/a", "r/c", "r/d"]);
    assert_eq!(keys(&db, &rows, true), ["r/d", "r/c", "r/a"]);
    assert_eq!(rows.get(Some(&db), "r/b").unwrap(), None);
    let mut scanned = Vec::new();
    db.scan("recs", "", |k, _| {
        scanned.push(k.to_owned());
        true
    })
    .unwrap();
    assert_eq!(scanned, ["j"]);

    release.send(()).unwrap();
    held.wait().unwrap();
    for ack in acks {
        ack.wait().unwrap();
    }
    assert!(db.pending().is_empty());
    db.close();

    let db = Db::open(&dir, "t.redb", &["recs", "rows"]).unwrap();
    assert_eq!(text(&db, "k"), None);
    assert_eq!(text(&db, "j").as_deref(), Some("jay"));
    assert_eq!(
        keys(&db, &RowTable::new("rows", false), false),
        ["r/a", "r/c", "r/d"]
    );
    db.close();
    drop(std::fs::remove_dir_all(&dir));
}

#[tokio::test(flavor = "current_thread")]
async fn a_read_off_the_loop_waits_for_the_queued_writes() {
    let dir = scratch("overlay-off-loop");
    let db = Db::open(&dir, "t.redb", &["recs"]).unwrap();
    let mut recs: Records<String> = Records::new("recs", "rec", 1);
    recs.insert("k".into(), "v".into());
    let (release, held) = hold(&db);
    let ack = db.submit(vec![recs.stage().unwrap()]).unwrap();
    let let_go = tokio::task::spawn_blocking(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        release.send(()).unwrap();
        held.wait()
    });
    let seen = db
        .read_off_loop(|read| {
            let mut n = 0;
            doze_store::scan(read, "recs", "", |_, _| {
                n += 1;
                true
            })?;
            Ok(n)
        })
        .await
        .unwrap();
    assert_eq!(seen, 1, "the read began after the write was committed");
    let_go.await.unwrap().unwrap();
    ack.await.unwrap();
    db.close();
    drop(std::fs::remove_dir_all(&dir));
}
