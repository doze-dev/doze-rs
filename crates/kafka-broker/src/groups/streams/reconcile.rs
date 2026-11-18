//! A member's way to its target (Kafka 4.4.0's streams `CurrentAssignmentBuilder`): what it holds and the target agree
//! on are kept; what it holds and is not to is revoked, and nothing new is given until it says it has let go of those;
//! a task another member (or, for standbys and warm-ups, another member of the same process) still holds is not given
//! until it is released. The member is at the target's epoch once nothing is left to revoke.

use std::collections::{BTreeMap, BTreeSet};

use super::tasks::{Task, Tasks};

/// Where a member is on its way to its target (Kafka's `MemberState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Stage {
    /// At its target, or at the epoch it was last given.
    #[default]
    Stable,
    /// Told to give up tasks and has not said it has.
    Unrevoked,
    /// Waiting for tasks other members have not released.
    Unreleased,
}

impl Stage {
    /// Kafka's byte for it, as the console shows it.
    #[must_use]
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Stable => "STABLE",
            Self::Unrevoked => "UNREVOKED_TASKS",
            Self::Unreleased => "UNRELEASED_TASKS",
        }
    }
}

/// What a member holds, and where it is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Held {
    pub(crate) epoch: i32,
    pub(crate) prev_epoch: i32,
    pub(crate) stage: Stage,
    /// What it may run now.
    pub(crate) tasks: Tasks,
    /// The epoch each active task was given at (a commit at an older epoch is held to it).
    pub(crate) given_at: BTreeMap<Task, i32>,
    /// What it has been told to give up and has not said it has.
    pub(crate) revoking: Tasks,
}

impl Held {
    fn moved_to(&mut self, epoch: i32) {
        self.prev_epoch = self.epoch;
        self.epoch = epoch;
    }
}

/// Who holds each task now, over every member (what they may run and what they are giving up), by process.
#[derive(Debug, Default)]
pub(crate) struct Holders<'a> {
    pub(crate) active: BTreeMap<&'a Task, &'a str>,
    pub(crate) standby: BTreeMap<&'a Task, BTreeSet<&'a str>>,
    pub(crate) warmup: BTreeMap<&'a Task, BTreeSet<&'a str>>,
}

impl<'a> Holders<'a> {
    /// Adds what one member on `process` holds.
    pub(crate) fn add(&mut self, process: &'a str, held: &'a Held) {
        for tasks in [&held.tasks, &held.revoking] {
            for t in &tasks.active {
                self.active.insert(t, process);
            }
            for t in &tasks.standby {
                self.standby.entry(t).or_default().insert(process);
            }
            for t in &tasks.warmup {
                self.warmup.entry(t).or_default().insert(process);
            }
        }
    }

    fn on(map: &BTreeMap<&'a Task, BTreeSet<&'a str>>, t: &Task, process: &str) -> bool {
        map.get(t).is_some_and(|p| p.contains(process))
    }
}

/// The member's step towards `target` at `target_epoch`, given what it says it owns (`None`: it did not say). `None`:
/// nothing changes.
#[must_use]
pub(crate) fn reconcile(
    held: &Held,
    process: &str,
    target: &Tasks,
    target_epoch: i32,
    owned: Option<&Tasks>,
    holders: &Holders<'_>,
) -> Option<Held> {
    let step = Step {
        held,
        process,
        target,
        target_epoch,
        owned,
        holders,
    };
    match held.stage {
        Stage::Stable if held.epoch != target_epoch => Some(step.next(held.epoch)),
        Stage::Stable => None,
        Stage::Unrevoked if step.not_released(&held.revoking) => None,
        Stage::Unrevoked => Some(step.next(held.epoch + 1)),
        Stage::Unreleased => Some(step.next(held.epoch)),
    }
}

struct Step<'s> {
    held: &'s Held,
    process: &'s str,
    target: &'s Tasks,
    target_epoch: i32,
    owned: Option<&'s Tasks>,
    holders: &'s Holders<'s>,
}

/// One role's split: kept, to revoke, to give, and whether something was held back.
fn split(
    current: &BTreeSet<Task>,
    target: &BTreeSet<Task>,
    blocked: impl Fn(&Task) -> bool,
) -> (BTreeSet<Task>, BTreeSet<Task>, BTreeSet<Task>, bool) {
    let kept: BTreeSet<Task> = current.intersection(target).cloned().collect();
    let revoke: BTreeSet<Task> = current.difference(&kept).cloned().collect();
    let mut give: BTreeSet<Task> = target.difference(&kept).cloned().collect();
    let before = give.len();
    give.retain(|t| !blocked(t));
    let held_back = give.len() != before;
    (kept, revoke, give, held_back)
}

impl Step<'_> {
    fn not_released(&self, revoking: &Tasks) -> bool {
        self.owned.is_none_or(|o| o.any_of(revoking))
    }

    /// Another member of this process holds `t` in `role`.
    fn other_here(&self, role: &BTreeMap<&Task, BTreeSet<&str>>, mine: &BTreeSet<Task>, t: &Task) -> bool {
        Holders::on(role, t, self.process) && !mine.contains(t)
    }

    fn active_here(&self, t: &Task) -> bool {
        self.holders.active.get(t).is_some_and(|p| *p == self.process)
    }

    fn next(&self, member_epoch: i32) -> Held {
        let (cur, h) = (&self.held.tasks, self.holders);
        let (mut active, active_revoke, active_give, a) = split(&cur.active, &self.target.active, |t| {
            h.active.contains_key(t)
                || self.other_here(&h.standby, &cur.standby, t)
                || self.other_here(&h.warmup, &cur.warmup, t)
        });
        let (mut standby, mut standby_revoke, mut standby_give, s) =
            split(&cur.standby, &self.target.standby, |t| {
                (self.active_here(t) && !cur.active.contains(t))
                    || Holders::on(&h.standby, t, self.process)
                    || self.other_here(&h.warmup, &cur.warmup, t)
            });
        let (mut warmup, mut warmup_revoke, mut warmup_give, w) =
            split(&cur.warmup, &self.target.warmup, |t| {
                self.active_here(t)
                    || self.other_here(&h.standby, &cur.standby, t)
                    || Holders::on(&h.warmup, t, self.process)
            });
        // An active task of its own to be a standby: given now (it stays among those to revoke as an active task).
        for t in &cur.active {
            if standby_give.remove(t) {
                standby.insert(t.clone());
            }
        }
        // A standby to be a warm-up, and the other way: changed in place.
        for t in &cur.standby {
            if warmup_give.remove(t) {
                warmup.insert(t.clone());
                standby_revoke.remove(t);
            }
        }
        for t in &cur.warmup {
            if standby_give.remove(t) {
                standby.insert(t.clone());
                warmup_revoke.remove(t);
            }
        }
        // A standby or warm-up to be active is never revoked: kept until the active task is given.
        for t in cur.standby.iter().chain(&cur.warmup) {
            if self.target.active.contains(t) {
                standby_revoke.remove(t);
                warmup_revoke.remove(t);
            }
        }
        let revoking = Tasks {
            active: active_revoke,
            standby: standby_revoke,
            warmup: warmup_revoke,
        };
        let revokes = !revoking.is_empty() && self.not_released(&revoking);
        for (mine, into) in [(&cur.standby, &mut standby), (&cur.warmup, &mut warmup)] {
            for t in mine {
                let promoted_now = !revokes && active_give.contains(t);
                if self.target.active.contains(t) && !promoted_now {
                    into.insert(t.clone());
                }
            }
        }
        let mut given_at: BTreeMap<Task, i32> = self
            .held
            .given_at
            .iter()
            .filter(|(t, _)| active.contains(*t) || (revokes && revoking.active.contains(*t)))
            .map(|(t, e)| (t.clone(), *e))
            .collect();
        let mut next = self.held.clone();
        let held_back = a || s || w;
        if revokes {
            next.stage = Stage::Unrevoked;
            next.moved_to(member_epoch);
            next.revoking = revoking;
        } else {
            let pending = !active_give.is_empty() || !standby_give.is_empty() || !warmup_give.is_empty();
            for t in &active_give {
                given_at.insert(t.clone(), self.target_epoch);
            }
            if pending {
                active.extend(active_give);
                standby.extend(standby_give);
                warmup.extend(warmup_give);
            }
            next.stage = if held_back { Stage::Unreleased } else { Stage::Stable };
            next.moved_to(self.target_epoch);
            next.revoking = Tasks::default();
        }
        next.tasks = Tasks {
            active,
            standby,
            warmup,
        };
        next.given_at = given_at;
        next
    }
}
