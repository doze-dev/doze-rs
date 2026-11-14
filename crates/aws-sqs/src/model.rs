//! What a queue and a message are. No wire types here: this is the state machine's vocabulary.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;

use serde::{Deserialize, Serialize};

use crate::fifo::FifoState;
use crate::msgattr::Attr;

/// The settings of a queue that have numeric meaning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Attrs {
    /// Seconds a received message stays hidden.
    pub(crate) visibility_timeout: u32,
    /// Seconds a sent message waits before it can be received.
    pub(crate) delay_seconds: u32,
    /// The largest message body, in bytes.
    pub(crate) max_message_size: u32,
    /// Seconds a message is kept.
    pub(crate) retention_seconds: u32,
    /// Seconds a receive waits for a message by default.
    pub(crate) wait_seconds: u32,
}

impl Default for Attrs {
    /// The defaults AWS reports for a new queue (recorded 2026-11-09).
    fn default() -> Self {
        Self {
            visibility_timeout: 30,
            delay_seconds: 0,
            max_message_size: 1_048_576,
            retention_seconds: 345_600,
            wait_seconds: 0,
        }
    }
}

/// One message in a queue.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Message {
    /// Its place in the queue's history: increasing, never reused. The key of its row on disk.
    pub(crate) ord: u64,
    pub(crate) id: String,
    pub(crate) body: String,
    pub(crate) md5_of_body: String,
    pub(crate) sent_ms: i64,
    /// The time from which it can be received.
    pub(crate) visible_at_ms: i64,
    pub(crate) receive_count: u32,
    pub(crate) first_received_ms: Option<i64>,
    /// Every receipt handle this message has been given. An old one still names the message: a delete with it succeeds.
    pub(crate) handles: Vec<String>,
    /// When the receive that put it in flight happened: a message can be kept in flight for twelve hours from this.
    pub(crate) received_ms: i64,
    /// The sender's message attributes.
    pub(crate) attributes: Vec<Attr>,
    /// The system attributes the sender set (only `AWSTraceHeader` exists).
    pub(crate) system: Vec<Attr>,
    pub(crate) group_id: Option<String>,
    pub(crate) dedup_id: Option<String>,
    /// A FIFO queue's sequence number for the message.
    pub(crate) sequence: Option<String>,
    /// The queue this message was moved from, when it is in a dead-letter queue.
    pub(crate) source_arn: Option<String>,
}

impl Message {
    /// Whether it is in flight: received, and not yet visible again.
    pub(crate) fn in_flight(&self, now_ms: i64) -> bool {
        !self.handles.is_empty() && self.visible_at_ms > now_ms
    }

    /// Whether it is waiting out its send delay.
    pub(crate) fn delayed(&self, now_ms: i64) -> bool {
        self.handles.is_empty() && self.visible_at_ms > now_ms
    }
}

/// A queue.
#[derive(Debug)]
pub(crate) struct Queue {
    pub(crate) name: String,
    pub(crate) attrs: Attrs,
    /// Attributes accepted and stored but not acted on yet, as the client wrote them.
    pub(crate) stored: BTreeMap<String, String>,
    pub(crate) created_secs: i64,
    pub(crate) modified_secs: i64,
    pub(crate) messages: Messages,
    pub(crate) tags: BTreeMap<String, String>,
    pub(crate) fifo: FifoState,
    /// When the queue was last purged: AWS allows one purge a minute.
    pub(crate) purged_ms: Option<i64>,
    /// The bytes last written for this queue's header and FIFO state, so a call writes them only when they changed.
    pub(crate) written: crate::persist::Written,
}

impl Queue {
    /// Whether `m` has passed this queue's retention: from then on it is gone, whether or not it has been removed yet.
    pub(crate) fn expired(&self, m: &Message, now_ms: i64) -> bool {
        m.sent_ms <= now_ms - i64::from(self.attrs.retention_seconds) * 1000
    }

    /// Removes the messages past their retention. Unless `thorough`, only when the first one is (the cheap check every call makes):
    /// a message moved in from another queue keeps its send time, so one further back can be older; it is counted out at once and
    /// removed by the next thorough pass, which comes no later than the first message's own expiry.
    pub(crate) fn expire(&mut self, now_ms: i64, thorough: bool) {
        let cutoff = now_ms - i64::from(self.attrs.retention_seconds) * 1000;
        let due = if thorough {
            self.messages.iter().any(|m| m.sent_ms <= cutoff)
        } else {
            self.messages.first().is_some_and(|m| m.sent_ms <= cutoff)
        };
        if due {
            self.messages.retain(|m| m.sent_ms > cutoff);
        }
    }

    /// When the first message passes its retention; `None` for an empty queue.
    pub(crate) fn expires_at(&self) -> Option<i64> {
        self.messages
            .first()
            .map(|m| m.sent_ms + i64::from(self.attrs.retention_seconds) * 1000)
    }
}

/// A queue's messages, oldest first. Reading is the slice's; every change goes through a method here, which remembers it
/// for the store, so a change cannot be made and forgotten.
#[derive(Default)]
pub(crate) struct Messages {
    /// Sorted by `ord`.
    items: Vec<Message>,
    next_ord: u64,
    /// Rows to write at the next flush.
    changed: BTreeSet<u64>,
    /// Rows to delete at the next flush.
    gone: BTreeSet<u64>,
}

impl std::fmt::Debug for Messages {
    /// The messages only: what is waiting to be written, and what ord comes next, are not part of what a queue holds.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(&self.items).finish()
    }
}

impl Deref for Messages {
    type Target = [Message];

    fn deref(&self) -> &[Message] {
        &self.items
    }
}

impl Messages {
    /// Messages read back from disk (already in `ord` order), with nothing yet to write.
    pub(crate) fn restored(items: Vec<Message>, next_ord: u64) -> Self {
        Self {
            items,
            next_ord,
            changed: BTreeSet::new(),
            gone: BTreeSet::new(),
        }
    }

    pub(crate) fn push(&mut self, mut message: Message) {
        message.ord = self.next_ord;
        self.next_ord += 1;
        self.changed.insert(message.ord);
        self.items.push(message);
    }

    /// The message at `index`, to be changed.
    pub(crate) fn get_mut(&mut self, index: usize) -> Option<&mut Message> {
        let message = self.items.get_mut(index)?;
        self.changed.insert(message.ord);
        Some(message)
    }

    /// The first message `is` accepts, to be changed.
    pub(crate) fn find_mut(&mut self, is: impl Fn(&Message) -> bool) -> Option<&mut Message> {
        let message = self.items.iter_mut().find(|m| is(m))?;
        self.changed.insert(message.ord);
        Some(message)
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&Message) -> bool) {
        let gone = &mut self.gone;
        self.items.retain(|m| {
            let kept = keep(m);
            if !kept {
                gone.insert(m.ord);
            }
            kept
        });
    }

    /// Takes out the messages `take` accepts and returns them, in order.
    pub(crate) fn remove_where(&mut self, mut take: impl FnMut(&Message) -> bool) -> Vec<Message> {
        let (out, keep): (Vec<Message>, Vec<Message>) = std::mem::take(&mut self.items)
            .into_iter()
            .partition(|m| take(m));
        self.items = keep;
        self.gone.extend(out.iter().map(|m| m.ord));
        out
    }

    /// Takes out up to `count` messages from the front.
    pub(crate) fn take_front(&mut self, count: usize) -> Vec<Message> {
        let out: Vec<Message> = self.items.drain(..count.min(self.items.len())).collect();
        self.gone.extend(out.iter().map(|m| m.ord));
        out
    }

    pub(crate) fn clear(&mut self) {
        self.gone.extend(self.items.iter().map(|m| m.ord));
        self.items.clear();
    }

    /// The messages to write: those changed and still here.
    pub(crate) fn to_write(&self) -> impl Iterator<Item = &Message> {
        self.changed.iter().filter_map(|ord| {
            let at = self.items.binary_search_by_key(ord, |m| m.ord).ok()?;
            self.items.get(at)
        })
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
