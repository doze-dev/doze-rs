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
    #[cfg_attr(not(feature = "inspect"), allow(dead_code))]
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
    fn other_here(
        &self,
        role: &BTreeMap<&Task, BTreeSet<&str>>,
        mine: &BTreeSet<Task>,
        t: &Task,
    ) -> bool {
        Holders::on(role, t, self.process) && !mine.contains(t)
    }

    fn active_here(&self, t: &Task) -> bool {
        self.holders
            .active
            .get(t)
            .is_some_and(|p| *p == self.process)
    }

    /// What the member keeps, is to give up and is to be given, role by role, with the changes of role made; and whether a
    /// task was held back for another member.
    fn split_all(&self) -> (Tasks, Tasks, Tasks, bool) {
        let (cur, h) = (&self.held.tasks, self.holders);
        let (active, active_revoke, active_give, a) =
            split(&cur.active, &self.target.active, |t| {
                h.active.contains_key(t)
                    || self.other_here(&h.standby, &cur.standby, t)
                    || self.other_here(&h.warmup, &cur.warmup, t)
            });
        let (standby, standby_revoke, standby_give, s) =
            split(&cur.standby, &self.target.standby, |t| {
                (self.active_here(t) && !cur.active.contains(t))
                    || Holders::on(&h.standby, t, self.process)
                    || self.other_here(&h.warmup, &cur.warmup, t)
            });
        let (warmup, warmup_revoke, warmup_give, w) =
            split(&cur.warmup, &self.target.warmup, |t| {
                self.active_here(t)
                    || self.other_here(&h.standby, &cur.standby, t)
                    || Holders::on(&h.warmup, t, self.process)
            });
        let mut kept = Tasks {
            active,
            standby,
            warmup,
        };
        let mut revoke = Tasks {
            active: active_revoke,
            standby: standby_revoke,
            warmup: warmup_revoke,
        };
        let mut give = Tasks {
            active: active_give,
            standby: standby_give,
            warmup: warmup_give,
        };
        // An active task of its own to be a standby: given now (it stays among those to revoke as an active task).
        for t in &cur.active {
            if give.standby.remove(t) {
                kept.standby.insert(t.clone());
            }
        }
        // A standby to be a warm-up, and the other way: changed in place.
        for t in &cur.standby {
            if give.warmup.remove(t) {
                kept.warmup.insert(t.clone());
                revoke.standby.remove(t);
            }
        }
        for t in &cur.warmup {
            if give.standby.remove(t) {
                kept.standby.insert(t.clone());
                revoke.warmup.remove(t);
            }
        }
        // A standby or warm-up to be active is never revoked: kept until the active task is given.
        for t in cur.standby.iter().chain(&cur.warmup) {
            if self.target.active.contains(t) {
                revoke.standby.remove(t);
                revoke.warmup.remove(t);
            }
        }
        (kept, revoke, give, a || s || w)
    }

    fn next(&self, member_epoch: i32) -> Held {
        let cur = &self.held.tasks;
        let (mut tasks, revoke, give, held_back) = self.split_all();
        let revokes = !revoke.is_empty() && self.not_released(&revoke);
        for (mine, into) in [
            (&cur.standby, &mut tasks.standby),
            (&cur.warmup, &mut tasks.warmup),
        ] {
            for t in mine {
                let promoted_now = !revokes && give.active.contains(t);
                if self.target.active.contains(t) && !promoted_now {
                    into.insert(t.clone());
                }
            }
        }
        let mut given_at: BTreeMap<Task, i32> = self
            .held
            .given_at
            .iter()
            .filter(|(t, _)| tasks.active.contains(*t) || (revokes && revoke.active.contains(*t)))
            .map(|(t, e)| (t.clone(), *e))
            .collect();
        let (stage, epoch, revoking) = if revokes {
            (Stage::Unrevoked, member_epoch, revoke)
        } else {
            for t in &give.active {
                given_at.insert(t.clone(), self.target_epoch);
            }
            tasks.active.extend(give.active);
            tasks.standby.extend(give.standby);
            tasks.warmup.extend(give.warmup);
            let stage = if held_back {
                Stage::Unreleased
            } else {
                Stage::Stable
            };
            (stage, self.target_epoch, Tasks::default())
        };
        Held {
            epoch,
            prev_epoch: self.held.epoch,
            stage,
            tasks,
            given_at,
            revoking,
        }
    }
}
