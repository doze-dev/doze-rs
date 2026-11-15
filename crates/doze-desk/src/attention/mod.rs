//! Needs you (Go `view/attention.go`, `serve/attention.go`, D19): what needs the person now, across every stack and the
//! machine. Red items are broken (something asked for is not happening); yellow ones are worth doing. Every surface
//! (the bell, the tabs' badges, alerts, the menu bar, ⌘K) reads this one list. An item leaves when its cause is gone;
//! "Not now" hides one until what it says changes.
//!
//! - [`stack_items`] and [`machine_items`] say what is wrong now;
//! - [`Steady`] keeps the list steady over time: an item that went missing is held for a grace (a crash loop passes
//!   through "starting" between tries, and that is not a fix), then shown "fixed" for a moment, but only when its stack
//!   is steady (not mid-action, not out of touch); a failed task stays red until a run of it succeeds;
//! - [`attention_of`] sorts, counts and drops what was put aside;
//! - [`away`] is what happened while the window was closed.

pub mod away;
mod items;
mod machine;
mod steady;
mod trouble;

use std::collections::BTreeMap;
use std::time::SystemTime;

use serde::Serialize;

pub use items::{StackFacts, StackFailure, stack_items};
pub use machine::{MachinePart, machine_items};
pub use steady::{FIXED_FOR, GRACE_FOR, StackState, Steady};
pub use trouble::stuck_waking;

/// Needs you, sorted and counted.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Attention {
    pub red: usize,
    pub yellow: usize,
    pub items: Vec<Item>,
    /// Items put aside with Not now.
    #[serde(skip_serializing_if = "is_zero")]
    pub hidden: usize,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // WHY: serde's skip_serializing_if passes a reference
fn is_zero(n: &usize) -> bool {
    *n == 0
}

impl Attention {
    /// The red items not yet fixed of the stack `key`: its tab's badge.
    #[must_use]
    pub fn red_in(&self, key: &str) -> usize {
        self.items
            .iter()
            .filter(|i| i.red && !i.fixed && i.key == key)
            .count()
    }
}

/// One thing that needs the person.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// Names the item across rebuilds: its kind, stack and instance.
    pub id: String,
    /// What it says, for Not now: a new print shows it again.
    pub print: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub red: bool,
    pub kind: &'static str,
    /// The stack's key; empty for the machine.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub key: String,
    /// The stack's name.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub stack: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub instance: String,
    pub title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// The detail is a program's own words, set in the code face.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub mono: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<Fix>,
    /// The page that explains it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show: Option<Place>,
    /// When it began, as far as doze knows.
    #[serde(skip)]
    pub since: Option<SystemTime>,
    /// The cause went away a moment ago: shown grey, then gone.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub fixed: bool,
}

/// One way out, as a button.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Fix {
    pub label: String,
    pub kind: FixKind,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub verb: &'static str,
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub pid: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub primary: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // WHY: serde's skip_serializing_if passes a reference
fn is_zero_i64(n: &i64) -> bool {
    *n == 0
}

/// What pressing a fix does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FixKind {
    /// A verb on the instance (`restart`, `task`), through its dialog.
    #[default]
    Do,
    /// A stack action (`up-last`, `restart`, `remove`), through its dialog.
    Stack,
    /// Stop the process holding a port (`pid`), then start the instance.
    Holder,
    /// The machine's setup, behind the administrator prompt.
    Repair,
    /// Clear what was left behind, all but data.
    Clean,
    /// Put a failed start away.
    Dismiss,
    /// Open doze.hcl (`path`) in the editor.
    Edit,
    /// Copy `text`.
    Copy,
    /// Go to the page that explains it.
    Show,
}

impl Fix {
    pub(crate) fn new(label: impl Into<String>, kind: FixKind) -> Fix {
        Fix {
            label: label.into(),
            kind,
            ..Fix::default()
        }
    }

    pub(crate) fn verb(mut self, verb: &'static str) -> Fix {
        self.verb = verb;
        self
    }

    pub(crate) fn primary(mut self) -> Fix {
        self.primary = true;
        self
    }
}

/// A page to go to: a stack's home, an instance, Settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Place {
    pub key: String,
    pub page: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub instance: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub section: &'static str,
}

/// Sorts the items, red first, newest first within each, fixed last; drops those put aside (`not_now`: id → the
/// print it had); counts (Go `AttentionOf`).
#[must_use]
pub fn attention_of(items: Vec<Item>, not_now: &BTreeMap<String, String>) -> Attention {
    let mut a = Attention::default();
    for it in items {
        if not_now
            .get(&it.id)
            .is_some_and(|p| !p.is_empty() && *p == it.print)
        {
            a.hidden += 1;
            continue;
        }
        if !it.fixed {
            if it.red {
                a.red += 1;
            } else {
                a.yellow += 1;
            }
        }
        a.items.push(it);
    }
    a.items.sort_by(|x, y| {
        x.fixed
            .cmp(&y.fixed)
            .then(y.red.cmp(&x.red))
            .then(y.since.cmp(&x.since))
    });
    a
}

/// Puts an item aside (`print`), or brings it back (`None`); keeps at most a hundred (Go `Service.NotNow`).
pub fn not_now(map: &mut BTreeMap<String, String>, id: &str, print: Option<&str>) {
    match print {
        Some(p) if !p.is_empty() => {
            map.insert(id.to_owned(), p.to_owned());
        }
        _ => {
            map.remove(id);
        }
    }
    while map.len() > 100 {
        let Some(old) = map.keys().find(|k| *k != id).cloned() else {
            break;
        };
        map.remove(&old);
    }
}

#[cfg(test)]
mod tests;
