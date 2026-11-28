//! Every open database of a data directory is written by one thread, which ends when the last one closes and starts again with the
//! next. Another data directory (another server in the same process: a test) has a writer of its own.
#![allow(clippy::unwrap_used)]

use doze_store::records::Records;
use doze_store::{Db, GroupCommit};

#[test]
fn the_databases_of_a_data_directory_share_one_writer_that_ends_with_the_last_close() {
    let dir = std::env::temp_dir().join(format!("doze-store-one-writer-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    assert_eq!(GroupCommit::writer_threads(), 0);
    // Six services' databases, and one of a region below one of them.
    let mut dirs: Vec<_> = (0..6).map(|i| dir.join(format!("s{i}"))).collect();
    dirs.push(dir.join("s0").join("regions").join("eu-west-1"));
    let dbs: Vec<Db> = dirs
        .iter()
        .map(|d| Db::open(d, "t.redb", &["rows"]).unwrap())
        .collect();
    assert_eq!(GroupCommit::writer_threads(), 1);
    let acks: Vec<_> = dbs
        .iter()
        .enumerate()
        .map(|(i, db)| {
            let mut rows = Records::<String>::new("rows", "row", 1);
            rows.insert("k".into(), format!("v{i}"));
            db.submit(vec![rows.stage().unwrap()]).unwrap()
        })
        .collect();
    for ack in acks {
        ack.wait().unwrap();
    }
    let other = Db::open(&dir.join("elsewhere").join("s0"), "t.redb", &["rows"]).unwrap();
    assert_eq!(GroupCommit::writer_threads(), 2, "another data directory");
    other.close();
    for (i, db) in dbs.into_iter().enumerate() {
        let read = db.read().unwrap();
        let rows = Records::<String>::load("rows", "row", 1, &read).unwrap();
        assert_eq!(rows.get("k").cloned(), Some(format!("v{i}")));
        drop(read);
        db.close();
    }
    assert_eq!(
        GroupCommit::writer_threads(),
        0,
        "no database open: no thread"
    );
    let again = Db::open(&dir.join("s0"), "t.redb", &["rows"]).unwrap();
    assert_eq!(GroupCommit::writer_threads(), 1);
    again.close();
    assert_eq!(GroupCommit::writer_threads(), 0);
    drop(std::fs::remove_dir_all(&dir));
}
