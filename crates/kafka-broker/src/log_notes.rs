//! What the cleaner and the log sweeper last did, kept only so that a console may say it.
//!
//! Tiny and additive: a note is written when a round of cleaning ran or a retention pass finished, never read by the broker
//! itself, and changes nothing the broker does. What is kept is bounded: one note per partition that has been cleaned (at
//! most [`MOST_PARTITIONS`], the oldest forgotten first), one for the last retention pass (at most [`MOST_DELETED`]
//! deletions listed), and two numbers. A broker that has cleaned nothing and swept nothing keeps nothing; the notes are
//! forgotten when the broker stops, as the page says.

#![cfg_attr(not(feature = "inspect"), allow(dead_code, unreachable_pub))]

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use kafka_wire::ids::TopicId;

/// The most partitions whose last round is remembered.
pub(crate) const MOST_PARTITIONS: usize = 512;
/// The most deletions of one retention pass that are listed.
pub(crate) const MOST_DELETED: usize = 64;

/// A round of cleaning that was put in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundNote {
    /// When it finished, in milliseconds since the epoch.
    pub at_ms: i64,
    /// How long it took, from the plan to the last segment in place.
    pub took_ms: u64,
    /// Records looked at, and records left.
    pub read: u64,
    pub kept: u64,
    /// Segments replaced (rewritten or removed).
    pub replaced: usize,
    /// Cleaning had got this far: the first offset not cleaned.
    pub clean_to: i64,
}

/// What the cleaner last did to one partition.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CleanerNote {
    pub last: Option<RoundNote>,
    /// A round is running, begun at this time.
    pub running_since_ms: Option<i64>,
    /// The last round that failed: when, and the broker's words.
    pub failed: Option<(i64, String)>,
}

/// A partition whose start retention moved up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedNote {
    pub topic: String,
    pub partition: i32,
    /// The log start before the pass and after it.
    pub from: i64,
    pub to: i64,
}

/// The last retention pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassNote {
    pub at_ms: i64,
    /// Partitions it looked at.
    pub looked: u32,
    /// The partitions whose start it moved (at most [`MOST_DELETED`]).
    pub deleted: Vec<DeletedNote>,
    /// More partitions than those listed had their start moved.
    pub more: u32,
}

/// The notes of one broker.
#[derive(Debug, Default)]
pub(crate) struct LogNotes {
    cleaner: Mutex<HashMap<(TopicId, i32), (i64, CleanerNote)>>,
    /// When the cleaner last looked at the logs (0: never since the broker began).
    looked_ms: AtomicI64,
    pass: Mutex<Option<PassNote>>,
    /// The partitions whose start retention has moved since the broker began, and when it last did.
    moved: AtomicU64,
    moved_at_ms: AtomicI64,
}

fn locked<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl LogNotes {
    /// The cleaner looked at the logs.
    pub(crate) fn looked(&self, at_ms: i64) {
        self.looked_ms.store(at_ms, Ordering::Relaxed);
    }

    pub(crate) fn looked_at(&self) -> Option<i64> {
        Some(self.looked_ms.load(Ordering::Relaxed)).filter(|t| *t > 0)
    }

    fn with(&self, topic: TopicId, partition: i32, at_ms: i64, f: impl FnOnce(&mut CleanerNote)) {
        let mut map = locked(&self.cleaner);
        if map.len() >= MOST_PARTITIONS && !map.contains_key(&(topic, partition)) {
            let oldest = map.iter().min_by_key(|(_, (t, _))| *t).map(|(k, _)| *k);
            if let Some(k) = oldest {
                map.remove(&k);
            }
        }
        let entry = map.entry((topic, partition)).or_default();
        entry.0 = at_ms;
        f(&mut entry.1);
    }

    /// A round was planned and is running.
    pub(crate) fn running(&self, topic: TopicId, partition: i32, at_ms: i64) {
        self.with(topic, partition, at_ms, |n| {
            n.running_since_ms = Some(at_ms)
        });
    }

    /// A round was put in place.
    pub(crate) fn round(&self, topic: TopicId, partition: i32, round: RoundNote) {
        self.with(topic, partition, round.at_ms, |n| {
            n.running_since_ms = None;
            n.failed = None;
            n.last = Some(round);
        });
    }

    /// A round failed (or was cut short by a stop).
    pub(crate) fn failed(&self, topic: TopicId, partition: i32, at_ms: i64, why: &str) {
        let words: String = why.chars().take(300).collect();
        self.with(topic, partition, at_ms, |n| {
            n.running_since_ms = None;
            n.failed = Some((at_ms, words));
        });
    }

    pub(crate) fn cleaner_of(&self, topic: TopicId, partition: i32) -> Option<CleanerNote> {
        locked(&self.cleaner)
            .get(&(topic, partition))
            .map(|(_, n)| n.clone())
    }

    /// A retention pass finished.
    pub(crate) fn pass(&self, note: PassNote) {
        let moved = u64::from(note.more) + note.deleted.len() as u64;
        if moved > 0 {
            self.moved.fetch_add(moved, Ordering::Relaxed);
            self.moved_at_ms.store(note.at_ms, Ordering::Relaxed);
        }
        *locked(&self.pass) = Some(note);
    }

    pub(crate) fn last_pass(&self) -> Option<PassNote> {
        locked(&self.pass).clone()
    }

    /// How many partitions' starts retention has moved since the broker began, and when it last did.
    pub(crate) fn moved(&self) -> (u64, Option<i64>) {
        let at = Some(self.moved_at_ms.load(Ordering::Relaxed)).filter(|t| *t > 0);
        (self.moved.load(Ordering::Relaxed), at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> TopicId {
        TopicId::from_bytes([n; 16])
    }

    fn round(at_ms: i64) -> RoundNote {
        RoundNote {
            at_ms,
            took_ms: 3,
            read: 10,
            kept: 4,
            replaced: 1,
            clean_to: 77,
        }
    }

    #[test]
    fn a_broker_that_has_done_nothing_keeps_nothing() {
        let n = LogNotes::default();
        assert_eq!(n.looked_at(), None);
        assert_eq!(n.cleaner_of(id(1), 0), None);
        assert_eq!(n.last_pass(), None);
        assert_eq!(n.moved(), (0, None));
    }

    #[test]
    fn a_round_replaces_the_running_and_failed_marks_and_is_remembered_per_partition() {
        let n = LogNotes::default();
        n.running(id(1), 0, 5);
        assert_eq!(n.cleaner_of(id(1), 0).unwrap().running_since_ms, Some(5));
        n.failed(id(1), 0, 6, "boom");
        let f = n.cleaner_of(id(1), 0).unwrap();
        assert_eq!(
            (f.running_since_ms, f.failed),
            (None, Some((6, "boom".into())))
        );
        n.round(id(1), 0, round(9));
        let d = n.cleaner_of(id(1), 0).unwrap();
        assert_eq!((d.last, d.failed), (Some(round(9)), None));
        assert_eq!(n.cleaner_of(id(1), 1), None, "another partition has none");
    }

    #[test]
    fn the_partitions_remembered_are_bounded_and_the_oldest_goes_first() {
        let n = LogNotes::default();
        for i in 0..(MOST_PARTITIONS as i32 + 10) {
            n.round(id(1), i, round(i64::from(i) + 1));
        }
        assert!(locked(&n.cleaner).len() <= MOST_PARTITIONS);
        assert_eq!(n.cleaner_of(id(1), 0), None, "the oldest was forgotten");
        assert!(n.cleaner_of(id(1), MOST_PARTITIONS as i32 + 9).is_some());
    }

    #[test]
    fn a_pass_is_the_last_one_and_the_moves_are_counted_across_passes() {
        let n = LogNotes::default();
        let moved = |at_ms| PassNote {
            at_ms,
            looked: 3,
            deleted: vec![DeletedNote {
                topic: "t".into(),
                partition: 0,
                from: 0,
                to: 10,
            }],
            more: 2,
        };
        n.pass(moved(100));
        n.pass(PassNote {
            at_ms: 200,
            looked: 3,
            deleted: Vec::new(),
            more: 0,
        });
        assert_eq!(n.last_pass().unwrap().at_ms, 200, "the last pass only");
        assert_eq!(n.moved(), (3, Some(100)), "and the moves are not forgotten");
    }
}
