//! Client telemetry (KIP-714): what a broker remembers of the clients that report on themselves.
//!
//! A client asks what to report (GetTelemetrySubscriptions), then pushes an OpenTelemetry payload on the interval it was
//! given (PushTelemetry). Apache Kafka hands the payloads to a plugin; this broker has none, so it keeps the last payload
//! of each client, as received, and does not read it: no byte of a payload is parsed, logged or shown. What a client
//! gets back depends only on the *gate* below, which is Kafka's: when it last asked, when it last pushed, and whether
//! its last push said it was the last (recorded: `telemetry-throttle`, `-interval`, `-terminating`, `-ask-again`,
//! `-instance-binding`, `-instance-binding-spaced`, `-broker-restart`).
//!
//! The store is bounded in entries (`MAX_INSTANCES`) and in bytes (`MAX_BYTES`), allocates nothing until a client
//! first asks, and has no timer: instances that went quiet are dropped the next time anyone calls.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use bytes::Bytes;

mod instance;

use instance::Instance;

/// How often a client is told to push, and the least time the gate allows between two of its requests. Short, for a
/// tool that looks at the last few minutes; Kafka's own default is five minutes.
pub(crate) const PUSH_INTERVAL: Duration = Duration::from_secs(5);
/// The largest payload a push may carry, and the most it may inflate to: Kafka's `telemetry.max.bytes`, which the
/// recordings show it advertising and accepting up to.
pub(crate) const MAX_PAYLOAD: usize = 1 << 20;
/// What a client is subscribed to: every metric of the standard set, which is what the prefix selects.
pub(crate) const REQUESTED_METRICS: &str = "org.apache.kafka.";
/// The compression types a push may use, in the broker's order of preference (zstd, lz4, gzip, snappy), as Apache Kafka
/// lists them. A push with none (0) is taken all the same, as Kafka takes it.
pub(crate) const ACCEPTED_COMPRESSION: [i8; 4] = [4, 3, 1, 2];

/// How many clients the broker remembers at once; the next evicts one (see [`Telemetry::keep`]).
const MAX_INSTANCES: usize = 128;
/// The most payload bytes held across all clients (one payload each, the last). Four payloads of the largest size.
const MAX_BYTES: usize = 4 * MAX_PAYLOAD;
/// How long a client that went quiet is remembered, and one that said it was terminating.
const QUIET_TTL: Duration = Duration::from_secs(600);
const TERMINATED_TTL: Duration = Duration::from_secs(60);

/// What the gate says to a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Within the rules.
    Accepted,
    /// It came before the interval had passed (THROTTLING_QUOTA_EXCEEDED).
    Early,
    /// It follows a push that said it was the client's last (INVALID_REQUEST).
    AfterTerminating,
}

/// The id of the subscription an instance is given: a function of the instance and of what it is subscribed to, as
/// Kafka's is (a checksum over the instance id, the metrics and the interval). So it is the same after a restart, and
/// a push that holds the id a client was given before one is accepted, as the recording of real Kafka shows.
pub(crate) fn subscription_id(instance: &[u8; 16]) -> i32 {
    let mut sum = crc32c::crc32c(instance);
    sum = crc32c::crc32c_append(sum, REQUESTED_METRICS.as_bytes());
    let millis = u32::try_from(PUSH_INTERVAL.as_millis()).unwrap_or(u32::MAX);
    sum = crc32c::crc32c_append(sum, &millis.to_be_bytes());
    i32::from_be_bytes(sum.to_be_bytes())
}

/// The clients that have reported, and the gate that keeps them to their interval.
#[derive(Default)]
pub(crate) struct Telemetry {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    instances: BTreeMap<[u8; 16], Instance>,
    /// The payload bytes held, the sum of every instance's.
    bytes: usize,
}

// Never prints a payload, or an instance id: only how much is held.
impl fmt::Debug for Telemetry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (instances, bytes) = self.held();
        f.debug_struct("Telemetry")
            .field("instances", &instances)
            .field("payload_bytes", &bytes)
            .finish()
    }
}

impl Telemetry {
    /// How many instances are remembered and how many payload bytes they hold.
    pub(crate) fn held(&self) -> (usize, usize) {
        let state = self.lock();
        (state.instances.len(), state.bytes)
    }

    /// A client asks for its subscription. Kafka answers a second request within the interval of the last one it took from
    /// the instance (a subscription or a push) THROTTLING_QUOTA_EXCEEDED, and a refused request postpones nothing.
    ///
    /// One request is answered though it is early: the one that follows a push refused for its subscription id or its
    /// compression type (see [`Telemetry::ask_again`]), which is the client doing what the refusal told it to. It is
    /// answered once and sets no clock, so the push straight after it is still early.
    pub(crate) fn subscribe(&self, id: [u8; 16], now: Instant) -> Verdict {
        let mut state = self.lock();
        let instance = state.touch(id, now);
        let asked = std::mem::take(&mut instance.ask_again);
        if instance
            .last_request()
            .is_some_and(|last| now.saturating_duration_since(last) < PUSH_INTERVAL)
        {
            return if asked {
                Verdict::Accepted
            } else {
                Verdict::Early
            };
        }
        instance.last_get = Some(now);
        Verdict::Accepted
    }

    /// A push arrives. The gate is applied before anything else of the push is looked at (recorded).
    ///
    /// The instance remembers whether the last push it was sent said it was terminating, whatever became of that push: so
    /// terminating pushes in a row are refused from the second on, a terminating push that was itself refused still
    /// closes the instance, and an ordinary push after one is refused once. A push is on time when a subscription was
    /// given since the last push that set the clock, or that push is an interval old; one on time sets the clock whatever
    /// becomes of it after. One that is not on time is early, unless it is terminating, which is the client's last word
    /// and is taken without setting the clock.
    pub(crate) fn arrive(&self, id: [u8; 16], terminating: bool, now: Instant) -> Verdict {
        let mut state = self.lock();
        let instance = state.touch(id, now);
        let was_terminating = std::mem::replace(&mut instance.said_terminating, terminating);
        // This push's own outcome decides whether the client is told to ask again.
        instance.ask_again = false;
        if was_terminating {
            return Verdict::AfterTerminating;
        }
        if instance.on_time(now) {
            instance.last_push = Some(now);
            Verdict::Accepted
        } else if terminating {
            Verdict::Accepted
        } else {
            Verdict::Early
        }
    }

    /// The push just let through by [`Telemetry::arrive`] was refused for its subscription id or its compression type.
    /// Both tell the client to ask for its subscription again, and that request is answered though it comes at once.
    pub(crate) fn ask_again(&self, id: &[u8; 16]) {
        if let Some(instance) = self.lock().instances.get_mut(id) {
            instance.ask_again = true;
        }
    }

    /// Keeps a payload: the instance's last, as received. When that takes the store over its byte budget the payloads of
    /// the instances heard from longest ago are let go (the instances stay: the gate still knows them).
    pub(crate) fn keep(&self, id: [u8; 16], payload: Bytes, now: Instant) {
        let mut state = self.lock();
        let instance = state.touch(id, now);
        let before = std::mem::replace(&mut instance.payload, payload);
        let after = instance.payload.len();
        state.bytes = state.bytes.saturating_sub(before.len()) + after;
        state.make_room(&id);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // A panic while holding the lock leaves nothing half done that the gate cannot live with.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl State {
    /// The instance `id`, made if it is new (a client the broker never heard of, perhaps one whose id survived a restart,
    /// is adopted), with `now` as the last time it was heard from. Quiet instances are dropped first, and when the store is
    /// full one is evicted: a client that said it was terminating, then the one heard from longest ago. An evicted client
    /// is simply unknown again: its next request is adopted, and costs it nothing but a throttle it would have had.
    fn touch(&mut self, id: [u8; 16], now: Instant) -> &mut Instance {
        self.forget_quiet(now);
        if !self.instances.contains_key(&id) && self.instances.len() >= MAX_INSTANCES {
            self.evict();
        }
        let instance = self.instances.entry(id).or_default();
        instance.heard = Some(now);
        instance
    }

    fn forget_quiet(&mut self, now: Instant) {
        let mut freed = 0;
        self.instances.retain(|_, i| {
            let ttl = if i.said_terminating {
                TERMINATED_TTL
            } else {
                QUIET_TTL
            };
            let keep = i
                .heard
                .is_none_or(|heard| now.saturating_duration_since(heard) <= ttl);
            if !keep {
                freed += i.payload.len();
            }
            keep
        });
        self.bytes = self.bytes.saturating_sub(freed);
    }

    fn evict(&mut self) {
        let victim = self
            .instances
            .iter()
            .min_by_key(|(_, i)| (!i.said_terminating, i.heard))
            .map(|(id, _)| *id);
        if let Some(instance) = victim.and_then(|id| self.instances.remove(&id)) {
            self.bytes = self.bytes.saturating_sub(instance.payload.len());
        }
    }

    /// Lets go of payloads, the least recently heard first and never `keep`'s, until the budget holds.
    fn make_room(&mut self, keep: &[u8; 16]) {
        while self.bytes > MAX_BYTES {
            let Some((_, oldest)) = self
                .instances
                .iter_mut()
                .filter(|(id, i)| *id != keep && !i.payload.is_empty())
                .min_by_key(|(_, i)| i.heard)
            else {
                return;
            };
            self.bytes = self.bytes.saturating_sub(oldest.payload.len());
            oldest.payload = Bytes::new();
        }
    }
}

#[cfg(test)]
mod tests;
