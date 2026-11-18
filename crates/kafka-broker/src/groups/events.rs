//! What a group has said of itself, kept for the console: a ring of notes per group, and a ledger that holds the rings of
//! the groups to a number.
//!
//! The ring belongs to the group's own task, which appends to it with no lock and no allocation until the group's first
//! note: an idle broker pays nothing, and a group that never rebalances never has one. Asking is a request to the task,
//! like any other, so what comes back is one consistent look. Only the ledger is shared: it is touched when a group
//! first says something and when a group's task ends, never by a note, and it is how the oldest group's ring is let
//! go when more than [`GROUPS_KEPT`] have one. Nothing is written to disk: a restart empties them all.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use super::machine::Millis;
use super::note::Note;

/// The most notes one group keeps.
pub(crate) const EVENTS_PER_GROUP: usize = 200;
/// The most groups that keep any.
pub(crate) const GROUPS_KEPT: usize = 100;
/// What one group's notes may weigh, by [`Note::weight`]: a group of many members says more with each note.
pub(crate) const RING_BYTES: usize = 64 * 1024;

/// A note, and how long before the look it was said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Recorded {
    pub(crate) ago_ms: u64,
    pub(crate) note: Note,
}

/// The groups that hold a ring, oldest first.
#[derive(Debug, Default)]
pub(crate) struct Ledger {
    held: Mutex<VecDeque<(u64, Arc<AtomicBool>)>>,
    next: AtomicU64,
}

impl Ledger {
    /// A place for a group's ring. The oldest group over the limit is told to let its own go.
    fn admit(self: &Arc<Self>) -> Slot {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let evicted = Arc::new(AtomicBool::new(false));
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.push_back((id, Arc::clone(&evicted)));
        while held.len() > GROUPS_KEPT {
            if let Some((_, flag)) = held.pop_front() {
                flag.store(true, Ordering::Relaxed);
            }
        }
        Slot {
            ledger: Arc::clone(self),
            id,
            evicted,
        }
    }

    /// How many groups hold a ring.
    #[cfg(test)]
    pub(crate) fn groups(&self) -> usize {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

/// A group's place in the ledger, given back when its task ends.
#[derive(Debug)]
struct Slot {
    ledger: Arc<Ledger>,
    id: u64,
    evicted: Arc<AtomicBool>,
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut held = self
            .ledger
            .held
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        held.retain(|(id, _)| *id != self.id);
    }
}

#[derive(Debug)]
struct Ring {
    slot: Slot,
    notes: VecDeque<Note>,
    weight: usize,
}

/// A group's notes, as its task keeps them.
#[derive(Debug)]
pub(crate) struct Events {
    ledger: Arc<Ledger>,
    ring: Option<Ring>,
}

#[cfg_attr(not(feature = "inspect"), allow(dead_code))]
impl Events {
    pub(crate) fn new(ledger: Arc<Ledger>) -> Self {
        Self { ledger, ring: None }
    }

    /// Keeps a note, the oldest going when the ring is full by number or by weight.
    pub(crate) fn push(&mut self, note: Note) {
        self.let_go_if_evicted();
        let ledger = &self.ledger;
        let ring = self.ring.get_or_insert_with(|| Ring {
            slot: ledger.admit(),
            notes: VecDeque::new(),
            weight: 0,
        });
        ring.weight += note.weight();
        ring.notes.push_back(note);
        while ring.notes.len() > EVENTS_PER_GROUP
            || (ring.weight > RING_BYTES && ring.notes.len() > 1)
        {
            if let Some(old) = ring.notes.pop_front() {
                ring.weight = ring.weight.saturating_sub(old.weight());
            }
        }
    }

    /// The notes, oldest first, each with how long before `now` it was said.
    pub(crate) fn look(&mut self, now: Millis) -> Vec<Recorded> {
        self.let_go_if_evicted();
        self.ring.as_ref().map_or_else(Vec::new, |ring| {
            ring.notes
                .iter()
                .map(|note| Recorded {
                    ago_ms: now.0.saturating_sub(note.at.0),
                    note: note.clone(),
                })
                .collect()
        })
    }

    fn let_go_if_evicted(&mut self) {
        if self
            .ring
            .as_ref()
            .is_some_and(|r| r.slot.evicted.load(Ordering::Relaxed))
        {
            self.ring = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::groups::note::{Cause, What, Why};

    fn note(at: u64, generation: i32) -> Note {
        Note {
            at: Millis(at),
            what: What::Began {
                why: Why::new(Cause::MemberJoined, None, 0),
                generation,
                after: 1,
                wait_ms: 1,
            },
        }
    }

    #[test]
    fn a_group_that_says_nothing_holds_nothing() {
        let ledger = Arc::new(Ledger::default());
        let mut events = Events::new(Arc::clone(&ledger));
        assert!(events.ring.is_none());
        assert!(events.look(Millis(10)).is_empty());
        assert_eq!(ledger.groups(), 0, "looking does not make a ring");
    }

    #[test]
    fn the_ring_keeps_the_newest_200_and_says_how_long_ago() {
        let ledger = Arc::new(Ledger::default());
        let mut events = Events::new(Arc::clone(&ledger));
        for i in 0..250 {
            events.push(note(u64::try_from(i).unwrap() * 10, i));
        }
        let seen = events.look(Millis(5_000));
        assert_eq!(seen.len(), EVENTS_PER_GROUP);
        let What::Began { generation, .. } = &seen[0].note.what else {
            panic!("not a rebalance")
        };
        assert_eq!(*generation, 50, "the oldest 50 went");
        assert_eq!(seen[0].ago_ms, 5_000 - 500);
        assert_eq!(seen.last().unwrap().ago_ms, 5_000 - 2_490);
    }

    #[test]
    fn a_ring_is_held_to_its_weight_as_well_as_its_number() {
        let ledger = Arc::new(Ledger::default());
        let mut events = Events::new(ledger);
        let member = kafka_wire::ids::MemberId::new("a-member-with-a-fairly-long-id").unwrap();
        let fat = |i: i32| Note {
            at: Millis(0),
            what: What::Settled {
                generation: i,
                holds: (0..8)
                    .map(|_| crate::groups::note::Hold {
                        member: member.clone(),
                        topics: (0..6)
                            .map(|t| (format!("topic-{t}").into(), (0..12).collect()))
                            .collect(),
                        total: 72,
                    })
                    .collect(),
                total: 8,
            },
        };
        for i in 0..1_000 {
            events.push(fat(i));
        }
        let ring = events.ring.as_ref().unwrap();
        assert!(ring.weight <= RING_BYTES, "{}", ring.weight);
        assert!(ring.notes.len() < EVENTS_PER_GROUP, "{}", ring.notes.len());
        assert!(!ring.notes.is_empty());
    }

    #[test]
    fn the_101st_group_to_speak_makes_the_oldest_let_go_of_its_ring() {
        let ledger = Arc::new(Ledger::default());
        let mut groups: Vec<Events> = (0..GROUPS_KEPT + 1)
            .map(|_| Events::new(Arc::clone(&ledger)))
            .collect();
        for (i, g) in groups.iter_mut().enumerate() {
            g.push(note(0, i32::try_from(i).unwrap()));
        }
        assert_eq!(ledger.groups(), GROUPS_KEPT);
        assert!(
            groups[0].look(Millis(1)).is_empty(),
            "the oldest group lost its story"
        );
        assert_eq!(groups[1].look(Millis(1)).len(), 1);
        assert_eq!(groups[GROUPS_KEPT].look(Millis(1)).len(), 1);
        // The evicted group may say something again: it is the newest then, and the next oldest goes.
        groups[0].push(note(2, 99));
        assert_eq!(groups[0].look(Millis(3)).len(), 1);
        assert!(groups[1].look(Millis(3)).is_empty());
        assert_eq!(ledger.groups(), GROUPS_KEPT);
    }

    #[test]
    fn a_group_that_ends_gives_its_place_back() {
        let ledger = Arc::new(Ledger::default());
        let mut a = Events::new(Arc::clone(&ledger));
        a.push(note(0, 1));
        assert_eq!(ledger.groups(), 1);
        drop(a);
        assert_eq!(ledger.groups(), 0);
    }
}
