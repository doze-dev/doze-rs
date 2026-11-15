//! Deletes free pages inside the file; only a compaction gives them back to the disk. On a real file, because the point is what the
//! disk sees.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use doze_store::Db;
use doze_store::records::Records;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Blob(Vec<u8>);

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("doze-store-{name}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    dir
}

#[test]
fn a_compaction_gives_back_what_deletes_freed_and_keeps_what_is_left() {
    let dir = scratch("compact");
    let mut db = Db::open(&dir, "t.redb", &["rows"]).unwrap();
    let mut rows: Records<Blob> = Records::new("rows", "row", 1);
    for i in 0..400 {
        rows.insert(format!("k{i:04}"), Blob(vec![7; 16 << 10]));
    }
    db.submit(vec![rows.stage().unwrap()])
        .unwrap()
        .wait()
        .unwrap();
    let full = db.space().unwrap();
    assert!(full.file > 6 << 20, "{full:?}");

    for i in 0..390 {
        rows.remove(&format!("k{i:04}"));
    }
    db.submit(vec![rows.stage().unwrap()])
        .unwrap()
        .wait()
        .unwrap();
    let emptied = db.space().unwrap();
    // The file keeps its size: redb only marks the pages free.
    assert!(emptied.file >= full.file / 2, "{emptied:?}");
    assert!(emptied.reclaimable() > 4 << 20, "{emptied:?}");

    let done = db.compact().unwrap();
    assert!(done.freed() > 4 << 20, "{done:?}");
    assert!(done.after < 1 << 20, "{done:?}");

    // What was kept is all there, and the writer works after the pause.
    let mut kept = 0;
    db.scan("rows", "", |_, _| {
        kept += 1;
        true
    })
    .unwrap();
    assert_eq!(kept, 10);
    rows.insert("again".to_owned(), Blob(vec![1; 10]));
    db.submit(vec![rows.stage().unwrap()])
        .unwrap()
        .wait()
        .unwrap();
    assert!(db.get("rows", "again").unwrap().is_some());
    db.close();
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn a_read_left_open_is_reported_not_waited_on() {
    let dir = scratch("busy");
    let mut db = Db::open(&dir, "t.redb", &["rows"]).unwrap();
    let read = db.read().unwrap();
    assert!(db.compact().is_err());
    drop(read);
    assert!(db.compact().is_ok());
    db.close();
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn a_close_gives_back_what_is_worth_it_and_leaves_a_small_gain_alone() {
    let dir = scratch("close");
    let db = Db::open(&dir, "t.redb", &["rows"]).unwrap();
    let mut rows: Records<Blob> = Records::new("rows", "row", 1);
    for i in 0..1600 {
        rows.insert(format!("k{i:05}"), Blob(vec![3; 64 << 10]));
    }
    db.submit(vec![rows.stage().unwrap()])
        .unwrap()
        .wait()
        .unwrap();
    for i in 0..1500 {
        rows.remove(&format!("k{i:05}"));
    }
    db.submit(vec![rows.stage().unwrap()])
        .unwrap()
        .wait()
        .unwrap();
    let before = db.space().unwrap();
    let started = std::time::Instant::now();
    db.close();
    println!(
        "closing compaction of {} MB took {:?}",
        before.file >> 20,
        started.elapsed()
    );
    let after = std::fs::metadata(dir.join("t.redb")).unwrap().len();
    assert!(after < before.file / 4, "{} -> {after}", before.file);

    // A few deletes: not worth a compaction.
    let db = Db::open(&dir, "t.redb", &["rows"]).unwrap();
    rows.remove("k01599");
    db.submit(vec![rows.stage().unwrap()])
        .unwrap()
        .wait()
        .unwrap();
    let size = std::fs::metadata(dir.join("t.redb")).unwrap().len();
    db.close();
    assert_eq!(std::fs::metadata(dir.join("t.redb")).unwrap().len(), size);
    drop(std::fs::remove_dir_all(&dir));
}
