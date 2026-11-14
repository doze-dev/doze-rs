//! The palette's commands (Go `view/commands.go`): the builders every row uses, the filter that ranks them, and the
//! palette narrowed with tab to a stack or a service.

use std::time::SystemTime;

use doze_display as words;

use super::instance::instance_commands;
use super::stack::stack_commands;
use super::{DoKind, Doing, Row, Scope, Section, Svc, key_of};
use crate::view::StackView;

/// A dim command row.
pub(crate) fn cmd(glyph: &'static str, label: String, also: &'static str, doing: Doing) -> Row {
    Row {
        glyph,
        tone: "dim",
        label,
        also,
        doing,
        ..Row::default()
    }
}

/// A doing of `kind` about the stack `key`.
pub(crate) fn doing(kind: DoKind, key: &str) -> Doing {
    Doing {
        kind,
        stack_key: key.to_owned(),
        ..Doing::default()
    }
}

/// A verb on one service, or on the whole stack when `target` is empty.
pub(crate) fn verb_on(key: &str, verb: &str, target: &str, ask: bool) -> Doing {
    Doing {
        verb: verb.to_owned(),
        target: target.to_owned(),
        ask,
        ..doing(DoKind::Verb, key)
    }
}

/// A page of one service (`instance`, `logs`, `activity`), or of the stack when `target` is empty.
pub(crate) fn page(key: &str, page: &str, target: &str) -> Doing {
    Doing {
        page: page.to_owned(),
        target: target.to_owned(),
        ..doing(DoKind::Page, key)
    }
}

/// Copies, opens or does something with `text`.
pub(crate) fn with_text(kind: DoKind, key: &str, text: &str, target: &str) -> Doing {
    Doing {
        text: text.to_owned(),
        target: target.to_owned(),
        ..doing(kind, key)
    }
}

/// A stack action.
pub(crate) fn stack_do(key: &str, verb: &str) -> Doing {
    Doing {
        verb: verb.to_owned(),
        ..doing(DoKind::Stack, key)
    }
}

/// The rows a query matches, best first, the label's matched characters marked; an empty query keeps them all, in
/// order. A row found by its other words (`also`) comes after every label match (Go `filterRows`).
#[must_use]
pub fn filter_rows(rows: Vec<Row>, query: &str) -> Vec<Row> {
    let query = query.trim();
    if query.is_empty() {
        return rows;
    }
    let mut hits: Vec<(usize, usize, Row)> = Vec::new();
    for (i, mut r) in rows.into_iter().enumerate() {
        if let Some(at) = words::fuzzy_match(query, &r.label) {
            let score = words::fuzzy_score(&at);
            r.at = at;
            hits.push((score, i, r));
        } else if !r.also.is_empty() && words_match(query, r.also) {
            hits.push((1 << 20, i, r));
        }
    }
    hits.sort_by_key(|h| (h.0, h.1));
    hits.into_iter().map(|h| h.2).collect()
}

/// Every word of the query starts a word of `also`.
fn words_match(query: &str, also: &str) -> bool {
    let also = also.to_lowercase();
    let words: Vec<&str> = also.split_whitespace().collect();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|q| words.iter().any(|w| w.starts_with(q)))
}

/// The palette inside a stack (`inst` `None`) or a service: its commands, and a stack's services to enter (Go
/// `scoped`). Returns the sections and the crumbs.
pub(crate) fn scoped(
    st: &StackView,
    inst: Option<&str>,
    query: &str,
    now: SystemTime,
) -> (Vec<Section>, Vec<String>) {
    let mut crumbs = vec![st.name.clone()];
    let snap = st.words();
    if let Some(inst) = inst {
        let Some(v) = snap.find(inst) else {
            return (Vec::new(), crumbs);
        };
        crumbs.push(inst.to_owned());
        let rows = filter_rows(instance_commands(st, Svc::of(st, v), now), query);
        let title = inst.to_owned();
        let out = if rows.is_empty() {
            Vec::new()
        } else {
            vec![Section { title, rows }]
        };
        return (out, crumbs);
    }
    let mut out = Vec::new();
    let rows = filter_rows(stack_commands(st), query);
    if !rows.is_empty() {
        out.push(Section {
            title: st.name.clone(),
            rows,
        });
    }
    if st.running {
        let key = key_of(st);
        let svc: Vec<Row> = words::rows(&snap)
            .into_iter()
            .map(|r| {
                let mut row = super::find::instance_row(st, &snap, r.v, now, "tab ›");
                row.enter = Some(Scope {
                    key: key.clone(),
                    name: Some(r.v.name.clone()),
                });
                row
            })
            .collect();
        let rows = filter_rows(svc, query);
        if !rows.is_empty() {
            out.push(Section {
                title: "Services".into(),
                rows,
            });
        }
    }
    (out, crumbs)
}

/// Two to four commands for where the person is: the service on screen, else the stack on screen (Go `suggested`).
pub(crate) fn suggested(
    stacks: &[&StackView],
    current: &str,
    inst: &str,
    now: SystemTime,
) -> Vec<Row> {
    let Some(st) = stacks.iter().find(|s| key_of(s) == current) else {
        return Vec::new();
    };
    if !inst.is_empty() {
        let snap = st.words();
        if let Some(v) = snap.find(inst) {
            return instance_commands(st, Svc::of(st, v), now)
                .into_iter()
                .take(4)
                .collect();
        }
    }
    // A stack's three: sleep, read the logs, or restart when it runs; up, start a few, or edit when it is stopped.
    let want: [&str; 3] = if st.running {
        ["Sleep everything", "All logs", "Restart"]
    } else {
        ["Up ", "Choose what starts", "Edit "]
    };
    let all = stack_commands(st);
    want.iter()
        .filter_map(|w| all.iter().find(|r| r.label.starts_with(w)).cloned())
        .collect()
}
