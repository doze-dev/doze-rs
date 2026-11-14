//! What the background chores (the cleaner, the retention sweep) last learnt of a partition, kept while it is closed, so that a
//! pass leaves alone a partition that cannot need it yet instead of opening it to find out.
//!
//! A verdict ("not due before T", or "not until it changes") holds while the partition is as it was when it was made: its
//! writer counts every change it makes (an append written, a trim, a round of cleaning put in place, a new config), the verdict
//! records the count it saw, and a verdict whose count is not the count now is not used. The writer is the only thing that
//! changes a partition's files, and it reads the count in the same step as it judges, so no change can come between the two.
//! The settings the verdict was judged by are part of it too: a changed `min.cleanable.dirty.ratio` or `retention.ms` asks
//! again.

use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

/// The chores that keep a verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Chore {
    Clean,
    Retain,
}

/// A chore's last verdict on a partition: nothing to do until `at_ms` (or, with `None`, until it changes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Verdict {
    /// The writer's count of changes when the verdict was made.
    pub(crate) changes: u64,
    /// What the settings it was judged by hash to ([`settings_print`]).
    pub(crate) settings: u64,
    /// The first time the chore could find something to do, if nothing is written meanwhile.
    pub(crate) at_ms: Option<i64>,
}

/// One partition's count of changes and the chores' verdicts on it. Kept by the registry across closes; the open partition's
/// writer holds it too, to count.
#[derive(Debug, Default)]
pub(crate) struct Upkeep {
    changes: AtomicU64,
    clean: Mutex<Option<Verdict>>,
    retain: Mutex<Option<Verdict>>,
}

impl Upkeep {
    /// How many changes the partition's writers have made to it in this run.
    pub(crate) fn changes(&self) -> u64 {
        self.changes.load(Ordering::Acquire)
    }

    /// Counts a change (called by the writer, in the step that makes it).
    pub(crate) fn changed(&self) {
        self.changes.fetch_add(1, Ordering::AcqRel);
    }

    fn slot(&self, chore: Chore) -> std::sync::MutexGuard<'_, Option<Verdict>> {
        match chore {
            Chore::Clean => &self.clean,
            Chore::Retain => &self.retain,
        }
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
    }

    /// Records a verdict.
    pub(crate) fn note(&self, chore: Chore, verdict: Verdict) {
        *self.slot(chore) = Some(verdict);
    }

    /// Whether the chore may leave the partition alone at `now_ms`: its last verdict was made on the partition as it is, by the
    /// settings in force, and named a time still to come (or none).
    pub(crate) fn skips(&self, chore: Chore, settings: u64, now_ms: i64) -> bool {
        let changes = self.changes();
        self.slot(chore).is_some_and(|v| {
            v.changes == changes && v.settings == settings && v.at_ms.is_none_or(|t| now_ms < t)
        })
    }
}

/// A print of the settings a verdict is judged by: equal settings, equal prints (within one run, which is as long as a
/// verdict lives).
pub(crate) fn settings_print(settings: &impl Hash) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    settings.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verdict_holds_until_its_time_a_change_or_other_settings() {
        let u = Upkeep::default();
        assert!(!u.skips(Chore::Clean, 1, 0), "no verdict yet");
        u.note(
            Chore::Clean,
            Verdict {
                changes: u.changes(),
                settings: 1,
                at_ms: Some(100),
            },
        );
        assert!(u.skips(Chore::Clean, 1, 99));
        assert!(!u.skips(Chore::Clean, 1, 100), "its time has come");
        assert!(!u.skips(Chore::Clean, 2, 50), "other settings");
        assert!(
            !u.skips(Chore::Retain, 1, 50),
            "another chore's verdict is its own"
        );
        u.changed();
        assert!(!u.skips(Chore::Clean, 1, 50), "written since");
        u.note(
            Chore::Retain,
            Verdict {
                changes: u.changes(),
                settings: 7,
                at_ms: None,
            },
        );
        assert!(
            u.skips(Chore::Retain, 7, i64::MAX),
            "never, until it changes"
        );
    }
}
