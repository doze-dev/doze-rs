//! The menu's Needs you section (Go `tray/needs.go`, the MenuBarNeeds board): a header, up to three red items each
//! with its first fix, and one line for the things worth doing.

use crate::attention::{Attention, Fix, FixKind, Item};

/// The section, as data. Empty when nothing is red or worth doing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Needs {
    /// "Needs you · 2"; empty when nothing is red.
    pub header: String,
    /// The red items, newest first, at most three.
    pub lines: Vec<NeedLine>,
    /// "and 3 things worth doing   ·   Open", or empty.
    pub worth: String,
}

/// One red item: its title with its stack, its detail as the first line of its submenu, and the fix the menu can
/// press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeedLine {
    pub title: String,
    pub detail: String,
    pub item: Item,
    /// The first fix that is not Show; `None` when there is none (the submenu then has Show alone).
    pub fix: Option<Fix>,
}

/// How many red items the menu lists.
const SHOWN_RED: usize = 3;

/// The section from the list as the bell has it (Go `NeedsOf`).
#[must_use]
pub fn needs_of(a: &Attention) -> Needs {
    let mut n = Needs::default();
    for it in a.items.iter().filter(|i| i.red && !i.fixed) {
        if n.lines.len() == SHOWN_RED {
            break;
        }
        let mut title = it.title.clone();
        if !it.stack.is_empty() {
            title = format!("{title}   ·   {}", it.stack);
        }
        n.lines.push(NeedLine {
            title,
            detail: it.detail.clone(),
            item: it.clone(),
            fix: it.fixes.iter().find(|f| f.kind != FixKind::Show).cloned(),
        });
    }
    if !n.lines.is_empty() {
        n.header = format!("Needs you · {}", a.red);
    }
    if a.yellow > 0 {
        let s = if a.yellow == 1 { "" } else { "s" };
        n.worth = format!("{} thing{s} worth doing   ·   Open", a.yellow);
        if !n.lines.is_empty() {
            n.worth = format!("and {}", n.worth);
        }
    }
    n
}

impl Needs {
    /// There is no section.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.worth.is_empty()
    }
}
