//! What a queue and a message are. No wire types here: this is the state machine's vocabulary.

use std::collections::BTreeMap;

use crate::msgattr::Attr;

/// The settings of a queue that have numeric meaning.
#[derive(Clone, Debug, PartialEq, Eq)]
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
#[derive(Debug)]
pub(crate) struct Message {
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
    pub(crate) messages: Vec<Message>,
    pub(crate) tags: BTreeMap<String, String>,
    /// When the queue was last purged: AWS allows one purge a minute.
    pub(crate) purged_ms: Option<i64>,
}
