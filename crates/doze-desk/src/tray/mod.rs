//! The menu bar, as data (Go `internal/desk/tray`, the MenuBar and MenuBarNeeds boards): the bat alone (never a
//! number), the alert mark when something needs you, one line per stack with a quiet second half, the Needs you
//! section above them, and each stack a submenu of what fits its state. The app turns a menu (from [`menu`]) into native items
//! and rebuilds them only when the menu's [`signature`] changes; the words come from `doze-display`, so the menu bar,
//! the CLI and the window say the same thing.
//!
//! - [`build`] and [`split`]: the stack lines and which of them the menu lists itself (Go `Build`, `Split`);
//! - [`needs_of`]: the Needs you section from the bell's list (Go `NeedsOf`);
//! - [`menu`]: the whole menu, commands and all (Go `app/tray.go`'s `menu`), with what Go's menu did not have: a
//!   stack's services, each with its wake or sleep and its logs, and the stack's All logs.

mod menu;
mod needs;
#[cfg(test)]
mod tests;

use std::time::SystemTime;

use doze_display as words;

use crate::conn::Conn;
use crate::view::StackView;

pub use menu::{Cmd, Entry, SERVICES_SHOWN, StackCmd, Urls, menu, signature, text};
pub use needs::{NeedLine, Needs, needs_of};

/// The menu bar, as data (Go `tray.Model`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Model {
    /// The alert mark: something crashed, or a task failed. The app sets it from Needs you's red count, as the bell
    /// counts it.
    pub needs_you: bool,
    /// One line for a hover: "doze · 4 awake".
    pub tooltip: String,
    /// One per stack: running ones first in name order, then the stopped ones most recently up first.
    pub lines: Vec<Line>,
}

/// One stack in the menu (Go `tray.Line`): its name, and a second half saying how it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub key: crate::StackKey,
    pub name: String,
    pub dir: String,
    pub running: bool,
    /// "4 awake · 551 MB", "search is crashing · try 2 of 5", or for a stopped stack "up 3d ago".
    pub sub: String,
    /// This stack is what the alert mark is about.
    pub trouble: bool,
}

/// How many stacks the menu lists before the rest go under More stacks: the menu bar is for a glance, and a long
/// native menu only scrolls with arrows at its edges.
const SHOWN_RUNNING: usize = 8;
const SHOWN_STOPPED: usize = 5;

/// The menu bar's model from what the hub knows at `now` (Go `Build`).
#[must_use]
pub fn build(stacks: &[StackView], now: SystemTime) -> Model {
    let mut m = Model {
        tooltip: "doze".into(),
        ..Model::default()
    };
    let mut awake = 0;
    let (mut running, mut stopped) = (Vec::new(), Vec::new());
    for st in stacks {
        let name = if st.name.is_empty() {
            "a stack".to_owned()
        } else {
            st.name.clone()
        };
        let mut l = Line {
            key: st.key.clone(),
            name,
            dir: st.dir.clone(),
            running: st.running,
            sub: String::new(),
            trouble: false,
        };
        if !st.running {
            l.sub = st.last_up.map_or_else(
                || "never up".to_owned(),
                |t| {
                    let ago = now.duration_since(t).unwrap_or_default();
                    format!("up {} ago", words::ago(ago))
                },
            );
            stopped.push((st.last_up, l));
            continue;
        }
        let snap = st.words();
        let t = words::tally(&snap.instances);
        if let Some(v) = snap.instances.iter().find(|v| words::troubled(v)) {
            // The first thing wrong is the line: what, and the try.
            l.trouble = true;
            m.needs_you = true;
            l.sub = format!("{} is {}", v.name, words::look(v).word());
            let mut b = words::because(v, &snap, now);
            // Not the countdown ("next in 3s"): it would rebuild the menu every second, maybe while it is open (Go's
            // line kept it and rebuilt). Needs you's detail still says it.
            if let Some(i) = b.find(" · next in ") {
                b.truncate(i);
            }
            if !b.is_empty() {
                l.sub = format!("{} · {b}", l.sub);
            }
        }
        awake += t.awake;
        if !l.trouble {
            l.sub = running_sub(st.link.conn, &t);
        }
        running.push(l);
    }
    running.sort_by(|a, b| a.name.cmp(&b.name));
    // Most recently up first; never up last; then by name.
    stopped.sort_by(|(ta, a), (tb, b)| tb.cmp(ta).then_with(|| a.name.cmp(&b.name)));
    if !running.is_empty() {
        m.tooltip = format!("doze · {awake} {}", words::Look::Awake.word());
    }
    m.lines = running;
    m.lines.extend(stopped.into_iter().map(|(_, l)| l));
    m
}

/// A running stack's quiet half: its connection when it is not live, else what is awake, dozing and waking, and
/// the memory of what serves.
fn running_sub(conn: Conn, t: &words::Tally) -> String {
    match conn {
        Conn::Connecting => "connecting…".into(),
        Conn::Down => "daemon not answering".into(),
        Conn::Live => {
            let mut s = format!("{} {}", t.awake, words::Look::Awake.word());
            if t.dozing > 0 {
                s = format!("{s} · {} {}", t.dozing, words::Look::Dozing.word());
            }
            if t.waking > 0 {
                s = format!("{s} · {} {}", t.waking, words::Look::Waking.word());
            }
            if t.ram > 0 {
                s = format!(
                    "{s} · {}",
                    words::mb(i64::try_from(t.ram).unwrap_or(i64::MAX))
                );
            }
            s
        }
    }
}

/// The lines the menu lists itself, and the rest for a More stacks submenu (Go `Split`): eight running, five
/// stopped, in [`build`]'s order; a stack in trouble is always listed.
#[must_use]
pub fn split(lines: &[Line]) -> (Vec<Line>, Vec<Line>) {
    let (mut shown, mut more) = (Vec::new(), Vec::new());
    let (mut running, mut stopped) = (0, 0);
    for l in lines {
        if l.trouble {
            shown.push(l.clone());
        } else if l.running && running < SHOWN_RUNNING {
            running += 1;
            shown.push(l.clone());
        } else if !l.running && stopped < SHOWN_STOPPED {
            stopped += 1;
            shown.push(l.clone());
        } else {
            more.push(l.clone());
        }
    }
    (shown, more)
}
