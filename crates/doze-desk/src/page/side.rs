//! The sidebar inside a stack: its instances under their groups, each with its look and a word or figure (Go
//! `view.SideOf`).

use doze_display::{self as words, Group, Kind, Look};

/// One instance in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Side {
    pub name: String,
    pub kind: Kind,
    pub group: Group,
    pub look: Look,
    /// `84 MB`, `dozing`, `asleep`, `done`; `!` in trouble.
    pub note: String,
    pub trouble: bool,
}

/// The sidebar's list, in the table's order.
pub fn side(snap: &words::Snapshot) -> Vec<Side> {
    words::rows(snap)
        .into_iter()
        .map(|r| {
            let v = r.v;
            let look = words::look(v);
            let trouble = words::troubled(v);
            let note = if trouble && v.kind != Kind::Task {
                "!".to_owned()
            } else if words::awake_now(v) && v.kind != Kind::Task {
                words::mb(i64::try_from(v.ram).unwrap_or(i64::MAX))
            } else {
                look.word().to_owned()
            };
            Side {
                name: v.name.clone(),
                kind: v.kind,
                group: r.group,
                look,
                note,
                trouble,
            }
        })
        .collect()
}
