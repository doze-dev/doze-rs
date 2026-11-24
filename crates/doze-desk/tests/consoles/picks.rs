//! A pick follows its row through the module's re-reads: by name, not by place.

use super::{ConsoleView, NodeKind, block, consoles, live};

#[test]
fn a_picked_session_stays_picked_when_the_module_reorders_its_rows() {
    use doze_desk::page::console::{Picked, selection};
    let all = consoles();
    let mut db = all["db"].clone();
    let mut picked = Picked::new();
    picked.insert("table:sessions".to_owned(), "4410".to_owned());
    let target = |c: &ConsoleView, picked: &Picked| {
        let sel = selection(c, "console:activity", picked);
        let p = live("db", "console:activity", c, &sel);
        let t = block(&p, NodeKind::Table);
        let shown = t
            .rows
            .iter()
            .find(|r| r.at == t.selected)
            .unwrap()
            .key
            .clone();
        let terminate = t.row_actions.iter().find(|a| a.id == "terminate").unwrap();
        (shown, terminate.resource.clone())
    };
    assert_eq!(target(&db, &picked), ("4410".to_owned(), "4410".to_owned()));
    // The next read comes in another order (postgres orders by state): the pick and the action follow the session.
    db.tables.get_mut("sessions").unwrap().reverse();
    assert_eq!(target(&db, &picked), ("4410".to_owned(), "4410".to_owned()));
    // The session ends: nothing is selected by that name, so the first row is, and the action names it.
    db.tables
        .get_mut("sessions")
        .unwrap()
        .retain(|r| r.cell("pid") != "4410");
    let sel = selection(&db, "console:activity", &picked);
    assert!(
        !sel.contains_key("table:sessions"),
        "a gone row is not selected"
    );
    let (shown, on) = target(&db, &picked);
    assert_eq!(shown, on, "the action is on the row shown");
    assert_ne!(on, "4410");
}
