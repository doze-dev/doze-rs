//! The List view's details pane: the selected instance in full, as the Go page drew it from `view.InstanceOf` (its
//! connect rows, why it is awake, what it needs, its last error). It reads the instance page's model
//! ([`crate::page::parts`]): the same rows, the same pills and the same crash words, so the pane and the page cannot
//! disagree, and the pane has the process's own IP and port under its address as the page and Go's service do.

use std::time::SystemTime;

use doze_display as words;
use serde::Serialize;

use super::Look;
use crate::page::parts;
pub use crate::page::parts::{Kv, Pill};
use crate::view::StackView;

/// The selected instance, for the pane.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Pane {
    pub name: String,
    pub kind: String,
    pub run: String,
    pub look: Look,
    pub because: String,
    /// How long it has been awake: `14m`.
    pub up: String,
    pub pid: i64,
    /// The connect card's rows.
    pub connect: Vec<Kv>,
    /// Why it is awake; `sleeps` is the sentence under them.
    pub holders: Vec<Pill>,
    pub sleeps: String,
    pub needs: Vec<NeedChip>,
    /// Set while it is crashing or has failed.
    pub crash: Option<Crash>,
}

/// One thing it needs, with how that is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct NeedChip {
    pub name: String,
    pub known: bool,
    pub look: Look,
    /// `✓` once it is up or done.
    pub glyph: &'static str,
    pub task: bool,
}

/// What a crashing or failed instance last said.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Crash {
    pub error: String,
    /// `try 3 of 5`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tries: String,
    /// `4 s`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub next_in: String,
}

/// The pane for the instance `name` of `st`, at `now`; `None` when the stack has none of that name.
pub fn pane(st: &StackView, name: &str, now: SystemTime) -> Option<Pane> {
    let snap = st.words();
    let (v, i) = st
        .instances
        .iter()
        .zip(&snap.instances)
        .find(|(v, _)| v.name == name)?;
    let (holders, sleeps) = parts::holders(i, st.idle_timeout);
    let mut p = Pane {
        name: v.name.clone(),
        kind: v.kind.clone(),
        run: v.run.clone(),
        look: Look::of(words::look(i)),
        because: words::because(i, &snap, now),
        pid: v.pid,
        connect: parts::with_direct(v, parts::connect_rows(v, i)),
        holders,
        sleeps,
        ..Pane::default()
    };
    if matches!(i.state, words::State::Active | words::State::Idle)
        && let Some(s) = i.started_at
    {
        p.up = words::ago(now.duration_since(s).unwrap_or_default());
    }
    p.needs = words::needs(i, &snap)
        .iter()
        .map(|n| NeedChip {
            name: n.name.to_owned(),
            known: n.known.is_some(),
            look: n.known.map(|k| Look::of(k.look)).unwrap_or_default(),
            glyph: n.glyph().unwrap_or(""),
            task: n.known.is_some_and(|k| k.ran)
                || snap
                    .find(n.name)
                    .is_some_and(|o| o.kind == words::Kind::Task),
        })
        .collect();
    if words::troubled(i) && v.kind != "task" {
        let c = parts::crash(i, now, Vec::new());
        p.crash = Some(Crash {
            error: c.error,
            tries: c.tries,
            next_in: c.next_in,
        });
    }
    Some(p)
}
