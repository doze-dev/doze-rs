//! One client as the gate knows it.

use std::fmt;
use std::time::Instant;

use bytes::Bytes;

use super::PUSH_INTERVAL;

/// What the gate remembers of a client instance, as Apache Kafka's `ClientMetricsInstance` does: when a subscription was
/// last given and a push last taken, that the last push said it was the client's last, and the last payload.
#[derive(Default)]
pub(super) struct Instance {
    /// The last time any request of the instance was heard (eviction and expiry go by it).
    pub(super) heard: Option<Instant>,
    pub(super) last_get: Option<Instant>,
    pub(super) last_push: Option<Instant>,
    pub(super) said_terminating: bool,
    /// The last push was refused in a way that tells the client to ask again.
    pub(super) ask_again: bool,
    /// The last payload, exactly as it arrived (compressed or not). Never read, only counted.
    pub(super) payload: Bytes,
}

impl Instance {
    /// When the instance's last accepted request, a subscription or a push, was.
    pub(super) fn last_request(&self) -> Option<Instant> {
        self.last_get.into_iter().chain(self.last_push).max()
    }

    /// Whether a push at `now` is on time: it is the first, or a subscription was given since the last push that set the
    /// clock, or that push is an interval old.
    pub(super) fn on_time(&self, now: Instant) -> bool {
        let Some(pushed) = self.last_push else {
            return true;
        };
        self.last_get.is_some_and(|got| pushed < got)
            || now.saturating_duration_since(pushed) >= PUSH_INTERVAL
    }
}

// A payload is the client's own words: its length is all that is ever printed.
impl fmt::Debug for Instance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Instance")
            .field("said_terminating", &self.said_terminating)
            .field("payload_bytes", &self.payload.len())
            .finish_non_exhaustive()
    }
}
