//! FIFO queues: the deduplication window, sequence numbers, and which messages a receive may take.
//!
//! What is here is what AWS's recordings establish (`test_fifo_*`, `test_content_based_deduplication_id`,
//! `test_receive_request_attempt_id`, recorded 2026-11-09). Throughput limits are stored and reported but do not change
//! behaviour: a laptop has no throughput quota to enforce.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::model::{Message, Queue};

/// How long a deduplication id is remembered.
pub(crate) const DEDUP_WINDOW_MS: i64 = 5 * 60 * 1000;

/// What one remembered deduplication id answers with, so a duplicate gets the original's answer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Remembered {
    /// The id, qualified by the group when the scope is per group.
    pub(crate) key: String,
    pub(crate) expires_ms: i64,
    pub(crate) message_id: String,
    pub(crate) md5_of_body: String,
    pub(crate) md5_of_attrs: Option<String>,
    pub(crate) md5_of_system: Option<String>,
    pub(crate) sequence: String,
}

/// A FIFO queue's bookkeeping. A standard queue has the empty one.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct FifoState {
    pub(crate) next_sequence: u64,
    pub(crate) window: Vec<Remembered>,
    /// The latest receive that carried a `ReceiveRequestAttemptId`: the id, when, and the messages it returned.
    pub(crate) attempt: Option<(String, i64, Vec<String>)>,
}

impl FifoState {
    /// The next sequence number: 18 digits, increasing, as AWS's look.
    pub(crate) fn next_sequence(&mut self) -> String {
        self.next_sequence += 1;
        (100_000_000_000_000_000_u64 + self.next_sequence).to_string()
    }
}

impl Queue {
    /// A FIFO queue is one created with `FifoQueue`: its name ends in `.fifo`.
    pub(crate) fn is_fifo(&self) -> bool {
        self.name.ends_with(".fifo")
    }

    /// Whether the queue makes a deduplication id from the body when the sender gives none.
    pub(crate) fn content_based(&self) -> bool {
        self.stored
            .get("ContentBasedDeduplication")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    }

    /// Whether ids are remembered per message group (`messageGroup`) rather than for the whole queue.
    pub(crate) fn dedup_per_group(&self) -> bool {
        self.stored
            .get("DeduplicationScope")
            .is_some_and(|v| v == "messageGroup")
    }

    /// The deduplication id a send carries: the sender's, else the SHA-256 of the body when the queue is content-based.
    pub(crate) fn dedup_id_for(&self, given: Option<&str>, body: &str) -> Option<String> {
        given.map(str::to_owned).or_else(|| {
            self.content_based()
                .then(|| aws_core::hex::encode(&Sha256::digest(body.as_bytes())))
        })
    }

    /// The remembered answer for a send with this id, if the window still holds it.
    pub(crate) fn remembered(
        &mut self,
        group: &str,
        dedup_id: &str,
        now_ms: i64,
    ) -> Option<Remembered> {
        self.fifo.window.retain(|r| r.expires_ms > now_ms);
        let key = self.window_key(group, dedup_id);
        self.fifo.window.iter().find(|r| r.key == key).cloned()
    }

    pub(crate) fn window_key(&self, group: &str, dedup_id: &str) -> String {
        if self.dedup_per_group() {
            format!("{group}\0{dedup_id}")
        } else {
            dedup_id.to_owned()
        }
    }
}

/// What a receive takes, and what it found past its retention on the way.
pub(crate) struct Picked {
    /// The `ord`s of the messages to hand over, in order.
    pub(crate) ords: Vec<u64>,
    /// Messages past the queue's retention: gone, to be removed.
    pub(crate) expired: Vec<u64>,
}

/// The messages a receive takes, in order, at most `limit`. The queue's messages must be promoted to `now_ms`.
///
/// A message is takeable when it is visible. On a FIFO queue a group with a message in flight is blocked, so its later
/// messages wait; and a group's messages are taken in order, so one that cannot be taken holds back those after it. Only the
/// visible messages are gone through, up to the `limit`th takeable one, and (on a FIFO queue) the hidden ones, for their groups.
pub(crate) fn pick(queue: &Queue, now_ms: i64, limit: usize) -> Picked {
    let mut picked = Picked {
        ords: Vec::new(),
        expired: Vec::new(),
    };
    let messages = &queue.messages;
    let live = |m: &&Message| !queue.expired(m, now_ms);
    // On a FIFO queue: the groups with a message in flight, and for each other group the first message that is not visible yet.
    let mut blocked: HashSet<&str> = HashSet::new();
    let mut held: HashMap<&str, u64> = HashMap::new();
    if queue.is_fifo() {
        for m in messages.not_visible(now_ms).filter(live) {
            let group = m.group_id.as_deref().unwrap_or("");
            if m.in_flight(now_ms) {
                blocked.insert(group);
            } else {
                let first = held.entry(group).or_insert(m.ord);
                *first = (*first).min(m.ord);
            }
        }
    }
    for m in messages.visible() {
        if picked.ords.len() == limit {
            break;
        }
        if queue.expired(m, now_ms) {
            picked.expired.push(m.ord);
            continue;
        }
        let group = m.group_id.as_deref().unwrap_or("");
        if blocked.contains(group) || held.get(group).is_some_and(|first| m.ord > *first) {
            continue;
        }
        picked.ords.push(m.ord);
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(group: &str, visible_at_ms: i64, in_flight: bool) -> Message {
        Message {
            ord: 0,
            id: String::new(),
            body: String::new(),
            md5_of_body: String::new(),
            sent_ms: 0,
            visible_at_ms,
            receive_count: 0,
            first_received_ms: None,
            handles: if in_flight {
                vec!["h".into()]
            } else {
                Vec::new()
            },
            received_ms: 0,
            attributes: Vec::new(),
            system: Vec::new(),
            group_id: Some(group.into()),
            dedup_id: None,
            sequence: None,
            source_arn: None,
        }
    }

    fn queue(name: &str, messages: impl IntoIterator<Item = Message>) -> Queue {
        let mut q = Queue {
            name: name.to_owned(),
            attrs: crate::model::Attrs::default(),
            stored: std::collections::BTreeMap::new(),
            created_secs: 0,
            modified_secs: 0,
            messages: crate::model::Messages::default(),
            tags: std::collections::BTreeMap::new(),
            fifo: FifoState::default(),
            purged_ms: None,
            written: crate::persist::Written::default(),
        };
        for m in messages {
            q.messages.push(m);
        }
        q
    }

    fn picked(q: &mut Queue, now_ms: i64, limit: usize) -> Vec<u64> {
        q.messages.promote(now_ms);
        pick(q, now_ms, limit).ords
    }

    #[test]
    fn a_receive_takes_a_groups_messages_in_order_and_skips_a_blocked_group() {
        let messages = || {
            [
                message("a", 1_000, true),
                message("a", 0, false),
                message("b", 0, false),
                message("b", 0, false),
            ]
        };
        let mut fifo = queue("q.fifo", messages());
        // Group a has one in flight, so only b's two can be taken.
        assert_eq!(picked(&mut fifo, 10, 10), [2, 3]);
        assert_eq!(picked(&mut fifo, 10, 1), [2]);
        // A standard queue takes any visible message.
        assert_eq!(picked(&mut queue("q", messages()), 10, 10), [1, 2, 3]);
    }

    #[test]
    fn a_message_that_cannot_be_taken_holds_back_those_after_it_in_its_group() {
        let mut q = queue(
            "q.fifo",
            [
                message("a", 100, false),
                message("a", 0, false),
                message("b", 0, false),
            ],
        );
        assert_eq!(picked(&mut q, 10, 10), [2]);
        assert_eq!(picked(&mut q, 100, 10), [0, 1, 2]);
    }

    #[test]
    fn a_message_past_its_retention_is_not_taken_and_is_named_for_removal() {
        let mut old = message("a", 0, false);
        old.sent_ms = -400_000_000;
        let mut q = queue("q", [old, message("a", 0, false)]);
        q.messages.promote(10);
        let p = pick(&q, 10, 10);
        assert_eq!((p.ords, p.expired), (vec![1], vec![0]));
    }

    #[test]
    fn sequence_numbers_increase() {
        let mut s = FifoState::default();
        let (a, b) = (s.next_sequence(), s.next_sequence());
        assert!(a < b && a.len() == 18 && b.len() == 18, "{a} {b}");
    }
}
