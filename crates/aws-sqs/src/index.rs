//! A queue's messages, kept by `ord`, with two indexes so a receive, a delete and a change of visibility do not go through every
//! message: the visible ones in arrival order (`ready`), and the hidden ones (in flight or delayed) by when they show
//! (`hidden`). Every change goes through a method here, which keeps the indexes and remembers the change for the store, so a
//! change cannot be made and forgotten.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::Message;

/// A queue's messages, oldest first.
#[derive(Default)]
pub(crate) struct Messages {
    items: BTreeMap<u64, Message>,
    /// Messages known to be visible, by `ord`: their time came at or before the last [`Messages::promote`].
    ready: BTreeSet<u64>,
    /// Every other message, by the time it shows and its `ord`. Some may have come due since the last promote.
    hidden: BTreeSet<(i64, u64)>,
    /// Messages received at least once (`receive_count > 0`): the only ones a redrive policy can move.
    received: BTreeSet<u64>,
    next_ord: u64,
    /// Rows to write at the next flush.
    changed: BTreeSet<u64>,
    /// Rows to delete at the next flush.
    gone: BTreeSet<u64>,
    /// A message was added, or one shows sooner than it did: a waiting receive may now find something.
    arrived: bool,
}

impl std::fmt::Debug for Messages {
    /// The messages only: the indexes and what is waiting to be written are not part of what a queue holds.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.items.values()).finish()
    }
}

impl Messages {
    /// Messages read back from disk, with nothing yet to write.
    pub(crate) fn restored(items: Vec<Message>, next_ord: u64) -> Self {
        let mut out = Self {
            next_ord,
            ..Self::default()
        };
        for m in items {
            out.index(&m);
            out.items.insert(m.ord, m);
        }
        out
    }

    fn index(&mut self, m: &Message) {
        self.hidden.insert((m.visible_at_ms, m.ord));
        if m.receive_count > 0 {
            self.received.insert(m.ord);
        }
    }

    fn unindex(&mut self, m: &Message) {
        if !self.ready.remove(&m.ord) {
            self.hidden.remove(&(m.visible_at_ms, m.ord));
        }
        self.received.remove(&m.ord);
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Every message, oldest first.
    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &Message> {
        self.items.values()
    }

    pub(crate) fn first(&self) -> Option<&Message> {
        self.items.values().next()
    }

    /// The `ord` the next message gets: every `ord` given so far is below it.
    pub(crate) fn next_ord(&self) -> u64 {
        self.next_ord
    }

    pub(crate) fn get(&self, ord: u64) -> Option<&Message> {
        self.items.get(&ord)
    }

    pub(crate) fn push(&mut self, mut message: Message) {
        message.ord = self.next_ord;
        self.next_ord += 1;
        self.changed.insert(message.ord);
        self.index(&message);
        self.items.insert(message.ord, message);
        self.arrived = true;
    }

    /// Changes the message `ord` names with `f`, and keeps the indexes.
    pub(crate) fn update<R>(&mut self, ord: u64, f: impl FnOnce(&mut Message) -> R) -> Option<R> {
        let mut m = self.items.remove(&ord)?;
        let (was_ready, before) = (self.ready.contains(&ord), m.visible_at_ms);
        self.unindex(&m);
        let out = f(&mut m);
        if m.visible_at_ms < before {
            self.arrived = true;
        }
        if was_ready && m.visible_at_ms <= before {
            self.ready.insert(ord);
            if m.receive_count > 0 {
                self.received.insert(ord);
            }
        } else {
            self.index(&m);
        }
        self.changed.insert(ord);
        self.items.insert(ord, m);
        Some(out)
    }

    pub(crate) fn remove(&mut self, ord: u64) -> Option<Message> {
        let m = self.items.remove(&ord)?;
        self.unindex(&m);
        self.gone.insert(ord);
        Some(m)
    }

    /// Moves the hidden messages whose time has come by `now_ms` to the visible ones.
    pub(crate) fn promote(&mut self, now_ms: i64) {
        while let Some(&(at, ord)) = self.hidden.first() {
            if at > now_ms {
                break;
            }
            self.hidden.pop_first();
            self.ready.insert(ord);
        }
    }

    /// The visible messages in arrival order (after a [`Messages::promote`] to the time asked about).
    pub(crate) fn visible(&self) -> impl Iterator<Item = &Message> {
        self.ready.iter().filter_map(|ord| self.items.get(ord))
    }

    /// The messages that are not visible at `now_ms` (in flight or delayed), soonest to show first.
    pub(crate) fn not_visible(&self, now_ms: i64) -> impl Iterator<Item = &Message> {
        self.hidden
            .range((now_ms.saturating_add(1), 0)..)
            .filter_map(|(_, ord)| self.items.get(ord))
    }

    /// When the next hidden message shows; `None` when none is hidden.
    pub(crate) fn next_visible(&self) -> Option<i64> {
        self.hidden.first().map(|(at, _)| *at)
    }

    /// The `ord`s of the messages received at least once.
    pub(crate) fn received(&self) -> impl Iterator<Item = u64> + '_ {
        self.received.iter().copied()
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&Message) -> bool) {
        let drop: Vec<u64> = self
            .items
            .values()
            .filter(|m| !keep(m))
            .map(|m| m.ord)
            .collect();
        for ord in drop {
            self.remove(ord);
        }
    }

    /// Takes out up to `count` messages from the front.
    pub(crate) fn take_front(&mut self, count: usize) -> Vec<Message> {
        let ords: Vec<u64> = self.items.keys().take(count).copied().collect();
        ords.into_iter()
            .filter_map(|ord| self.remove(ord))
            .collect()
    }

    pub(crate) fn clear(&mut self) {
        self.gone.extend(self.items.keys().copied());
        self.items.clear();
        self.ready.clear();
        self.hidden.clear();
        self.received.clear();
    }

    /// Whether something arrived since the last call, which forgets it.
    pub(crate) fn take_arrived(&mut self) -> bool {
        std::mem::take(&mut self.arrived)
    }

    /// The messages to write: those changed and still here.
    pub(crate) fn to_write(&self) -> impl Iterator<Item = &Message> {
        self.changed.iter().filter_map(|ord| self.items.get(ord))
    }

    /// The ords whose rows are to be deleted.
    pub(crate) fn to_delete(&self) -> impl Iterator<Item = u64> + '_ {
        self.gone.iter().copied()
    }

    pub(crate) fn has_changes(&self) -> bool {
        !self.changed.is_empty() || !self.gone.is_empty()
    }

    /// The changes have been handed to the store.
    pub(crate) fn settle(&mut self) {
        self.changed.clear();
        self.gone.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(visible_at_ms: i64) -> Message {
        Message {
            visible_at_ms,
            ..Message::default()
        }
    }

    #[test]
    fn a_message_moves_between_the_indexes_as_its_time_comes_and_changes() {
        let mut m = Messages::default();
        m.push(at(0));
        m.push(at(500));
        m.push(at(100));
        assert!(m.take_arrived(), "a push is an arrival");
        m.promote(100);
        assert_eq!(m.visible().map(|x| x.ord).collect::<Vec<_>>(), [0, 2]);
        assert_eq!(m.next_visible(), Some(500));
        // Hidden again (received): out of the visible ones, and not an arrival.
        m.update(0, |x| {
            x.visible_at_ms = 1_000;
            x.receive_count = 1;
        });
        assert!(!m.take_arrived());
        assert_eq!(m.visible().map(|x| x.ord).collect::<Vec<_>>(), [2]);
        assert_eq!(m.received().collect::<Vec<_>>(), [0]);
        assert_eq!(m.not_visible(100).count(), 2);
        // Shown sooner: an arrival.
        m.update(0, |x| x.visible_at_ms = 100);
        assert!(m.take_arrived());
        m.promote(100);
        assert_eq!(m.visible().map(|x| x.ord).collect::<Vec<_>>(), [0, 2]);
        assert_eq!(m.remove(0).map(|x| x.ord), Some(0));
        assert_eq!(m.received().count(), 0);
        assert_eq!(m.take_front(5).len(), 2);
        assert!(m.is_empty() && m.next_visible().is_none() && m.visible().count() == 0);
    }
}
