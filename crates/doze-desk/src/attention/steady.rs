//! Keeping Needs you steady over time (Go `serve.Service.Attention`'s bookkeeping). An item that leaves is shown
//! "fixed" only when doze can tell it was: it has been gone a few seconds, its stack is steady (not mid-action, not out
//! of touch, still there), and the person did not put it aside, dismiss it or remove its stack. Otherwise it is carried
//! as it was, or leaves quietly.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, SystemTime};

use super::{Attention, Fix, FixKind, Item, attention_of};
use crate::conn::Conn;
use crate::view::StackView;

/// How long an item may be missing before it counts as gone: a crash loop passes through "starting" between tries,
/// and that is not a fix.
pub const GRACE_FOR: Duration = Duration::from_secs(5);

/// How long an item whose cause went away stays, grey, saying "fixed", before it leaves.
pub const FIXED_FOR: Duration = Duration::from_secs(8);

/// What a missing item of a stack may conclude.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackState {
    /// Live and steady: a missing item was fixed.
    Live,
    /// Stopped: its services' trouble went with it, quietly.
    Stopped,
    /// Mid-action or out of touch: keep the item as it was.
    Carry,
}

/// The list's memory between builds.
#[derive(Debug, Default)]
pub struct Steady {
    /// Every item of the last build, by id (fixed ones aside).
    last: BTreeMap<String, Item>,
    /// Items missing, and since when, within their grace.
    absent: BTreeMap<String, SystemTime>,
    /// Items shown fixed, and since when.
    gone: BTreeMap<String, (Item, SystemTime)>,
    /// Items the person dismissed or removed: they leave without "fixed".
    quiet: BTreeSet<String>,
}

const STOPPED_SUFFIX: &str = " · its stack is stopped";

impl Steady {
    /// Says the items `matching` leave without "fixed": the person dismissed or removed them.
    pub fn quiet(&mut self, matching: impl Fn(&Item) -> bool) {
        for (id, it) in &self.last {
            if matching(it) {
                self.quiet.insert(id.clone());
            }
        }
    }

    /// Whether an item waits out its grace or its "fixed": the list must be built again in a moment even if nothing
    /// else changes.
    #[must_use]
    pub fn waiting(&self) -> bool {
        !self.absent.is_empty() || !self.gone.is_empty()
    }

    /// The list now: `items` (what is wrong now, from [`super::stack_items`] and [`super::machine_items`]) kept
    /// steady against the last build. `busy` says a stack is mid-action; `not_now` is what was put aside.
    pub fn step(
        &mut self,
        mut items: Vec<Item>,
        stacks: &[StackView],
        busy: &dyn Fn(&str) -> bool,
        not_now: &BTreeMap<String, String>,
        now: SystemTime,
    ) -> Attention {
        let state_of = |key: &str| -> Option<StackState> {
            if key.is_empty() {
                return Some(StackState::Live);
            }
            let st = stacks
                .iter()
                .find(|s| s.key.dir().to_string_lossy() == key)?;
            Some(if busy(key) || (st.running && st.link.conn != Conn::Live) {
                StackState::Carry
            } else if st.running {
                StackState::Live
            } else {
                StackState::Stopped
            })
        };
        let mut present: BTreeSet<String> = items.iter().map(|i| i.id.clone()).collect();
        for id in &present {
            self.absent.remove(id);
            self.quiet.remove(id);
        }
        let last = std::mem::take(&mut self.last);
        for (id, prev) in last {
            if present.contains(&id) {
                continue;
            }
            let Some(state) = state_of(&prev.key) else {
                continue; // its stack was removed: nothing to say
            };
            if let Some(kept) = self.missing(prev, state, stacks, not_now, now) {
                present.insert(id);
                items.push(kept);
            }
        }
        self.gone.retain(|id, (_, at)| {
            !present.contains(id) && now.duration_since(*at).unwrap_or_default() <= FIXED_FOR
        });
        items.extend(self.gone.values().map(|(it, _)| it.clone()));
        self.last = items
            .iter()
            .filter(|i| !i.fixed)
            .map(|i| (i.id.clone(), i.clone()))
            .collect();
        attention_of(items, not_now)
    }

    /// An item of the last build missing from this one: kept (as it was, or reworded), or concluded (fixed, or gone
    /// quietly; `None`).
    fn missing(
        &mut self,
        mut prev: Item,
        state: StackState,
        stacks: &[StackView],
        not_now: &BTreeMap<String, String>,
        now: SystemTime,
    ) -> Option<Item> {
        let id = prev.id.clone();
        match (state, prev.kind) {
            (StackState::Carry, _) => return Some(prev),
            (StackState::Stopped, "task") => {
                // It cannot run again until its stack is up: say so, keep it red.
                let name = stacks
                    .iter()
                    .find(|s| s.key.dir().to_string_lossy() == prev.key)
                    .map_or("", |s| s.name.as_str());
                prev.detail = format!("{}{STOPPED_SUFFIX}", trim_stopped(&prev.detail));
                prev.fixes = vec![
                    Fix::new(format!("Up {name}"), FixKind::Stack)
                        .verb("up-last")
                        .primary(),
                ];
                return Some(prev);
            }
            (StackState::Live, "task") => {
                let st = stacks
                    .iter()
                    .find(|s| s.key.dir().to_string_lossy() == prev.key)?;
                // No longer in doze.hcl: nothing to run.
                let v = st.instance(&prev.instance)?;
                if v.task.is_none() {
                    // Up again with no run since: still the last word on it.
                    prev.detail = trim_stopped(&prev.detail).to_owned();
                    prev.fixes = vec![
                        Fix::new("Run again", FixKind::Do).verb("task").primary(),
                        Fix::new("Output", FixKind::Show),
                    ];
                    return Some(prev);
                }
            }
            (StackState::Stopped, _) if !prev.instance.is_empty() => return None,
            _ if self.quiet.remove(&id) => return None,
            _ => {}
        }
        let since = *self.absent.entry(id.clone()).or_insert(now);
        if now.duration_since(since).unwrap_or_default() < GRACE_FOR {
            return Some(prev);
        }
        self.absent.remove(&id);
        if self.quiet.remove(&id)
            || not_now
                .get(&id)
                .is_some_and(|p| !p.is_empty() && *p == prev.print)
        {
            return None;
        }
        prev.fixed = true;
        prev.fixes.clear();
        self.gone.insert(id, (prev, now));
        None
    }
}

/// A task item's detail without " · its stack is stopped".
fn trim_stopped(d: &str) -> &str {
    d.strip_suffix(STOPPED_SUFFIX).unwrap_or(d)
}
