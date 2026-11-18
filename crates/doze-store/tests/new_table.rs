//! A table a later version adds: a file made before it existed opens, the table reads as empty, and the first write makes it.
#![allow(clippy::unwrap_used)]

use doze_store::Db;
use doze_store::records::Records;

#[test]
fn a_table_added_later_reads_empty_and_is_made_by_the_first_write() {
    let dir = std::env::temp_dir().join(format!("doze-store-new-table-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    let db = Db::open(&dir, "t.redb", &["old"]).unwrap();
    db.close();

    let db = Db::open(&dir, "t.redb", &["old", "new"]).unwrap();
    let read = db.read().unwrap();
    assert!(Records::<String>::load("new", "row", 1, &read).is_err());
    let mut rows = Records::<String>::load_or_new("new", "row", 1, &read).unwrap();
    drop(read);
    assert!(rows.is_empty());
    rows.insert("k".into(), "v".into());
    db.submit(vec![rows.stage().unwrap()])
        .unwrap()
        .wait()
        .unwrap();
    let read = db.read().unwrap();
    let again = Records::<String>::load_or_new("new", "row", 1, &read).unwrap();
    assert_eq!(again.get("k").map(String::as_str), Some("v"));
    drop(read);
    db.close();
    drop(std::fs::remove_dir_all(&dir));
}
