//! A pick follows its row through the module's re-reads: by identity, not by place, and never onto another row.
#![allow(clippy::indexing_slicing)]

use super::{ConsoleView, NodeKind, block, consoles, live};
use doze_desk::page::console::{Picked, gone, row_ident, selection};

const TAB: &str = "console:activity";
const TABLE: &str = "sessions";

/// The row selected and the row the Terminate action names, as the tab draws them.
fn target(c: &ConsoleView, picked: &Picked) -> (String, String) {
    let sel = selection(c, TAB, picked);
    let p = live("db", TAB, c, &sel);
    let t = block(&p, NodeKind::Table);
    let shown = t.rows.iter().find(|r| r.at == t.selected).unwrap();
    let terminate = t.row_actions.iter().find(|a| a.id == "terminate").unwrap();
    assert_eq!(
        terminate.row, shown.ident,
        "the action names the row it shows"
    );
    (shown.key.clone(), terminate.resource.clone())
}

fn pick(c: &ConsoleView, pid: &str) -> Picked {
    let rows = &c.tables["sessions"];
    let i = rows.iter().position(|r| r.cell("pid") == pid).unwrap();
    Picked::from([(TABLE.to_owned(), row_ident(rows, i, "pid"))])
}

#[test]
fn a_picked_session_stays_picked_when_the_module_reorders_its_rows() {
    let all = consoles();
    let mut db = all["db"].clone();
    let picked = pick(&db, "4410");
    assert_eq!(target(&db, &picked), ("4410".to_owned(), "4410".to_owned()));
    // The next read comes in another order (postgres orders by state): the pick and the action follow the session.
    db.tables.get_mut("sessions").unwrap().reverse();
    assert_eq!(target(&db, &picked), ("4410".to_owned(), "4410".to_owned()));
    assert!(gone(&db, TAB, &picked).is_empty());
    // The session ends: nothing is selected by that identity, and the table says its pick is gone, so the app runs
    // nothing on whichever row is drawn first now.
    db.tables
        .get_mut("sessions")
        .unwrap()
        .retain(|r| r.cell("pid") != "4410");
    assert!(!selection(&db, TAB, &picked).contains_key(TABLE));
    assert_eq!(gone(&db, TAB, &picked), [TABLE.to_owned()]);
}

#[test]
fn rows_of_the_same_name_are_different_picks() {
    let all = consoles();
    let mut db = all["db"].clone();
    // Two rows named alike (a table led by something that repeats, such as a time).
    for r in db.tables.get_mut("sessions").unwrap().iter_mut().take(4) {
        r.cells.insert("pid".to_owned(), "4400".to_owned());
    }
    let rows = &db.tables["sessions"];
    let second = row_ident(rows, 1, "pid");
    assert_ne!(second, row_ident(rows, 0, "pid"));
    let picked = Picked::from([(TABLE.to_owned(), second.clone())]);
    assert_eq!(
        selection(&db, TAB, &picked).get(TABLE),
        Some(&1),
        "the second, not the first"
    );
    let sel = selection(&db, TAB, &picked);
    let p = live("db", TAB, &db, &sel);
    let t = block(&p, NodeKind::Table);
    assert!(t.row_actions.iter().all(|a| a.row == second));
}
