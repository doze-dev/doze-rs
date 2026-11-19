//! The Live record: the last [`CAPACITY`] finished AWS API calls, as the gateway saw them.
//!
//! The gateway tells the [`Recorder`] about each answered request on its request path ([`aws_gateway::Observer`]). What that
//! costs a request is bounded: one lock, five short strings copied (each cut at a fixed length), one slot of the ring written,
//! and a number published to whoever is tailing. Nothing here owns a timer or a task: with no call there is no wake-up, and with
//! no reader a call wakes nobody.
//!
//! What it holds is bounded too. A record is [`RECORD_BYTES`] in the ring plus its text, which is at most [`MOST_TEXT`]:
//! [`MOST_BYTES`] in all when every record is as long as it may be (about 0.9 MB), and about 0.25 MB for ordinary calls (a 36-byte
//! request id, a 20-byte key, a region, no resource). The ring is empty until the first call and grows to its capacity; it is
//! never allocated ahead.
//!
//! Every record has a sequence number, 1 for the first call of this process. A reader says the last one it has and is given what
//! came after ([`Recorder::after`]), and is told how many it missed if the ring has moved past it.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use aws_gateway::{Finished, Observer};
use tokio::sync::watch;

/// The most calls held.
pub const CAPACITY: usize = 1024;
/// The most bytes of each text of a record that are kept.
pub const MOST_REQUEST_ID: usize = 64;
/// See [`MOST_REQUEST_ID`].
pub const MOST_CALLER: usize = 128;
/// See [`MOST_REQUEST_ID`].
pub const MOST_REGION: usize = 32;
/// See [`MOST_REQUEST_ID`].
pub const MOST_CODE: usize = 96;
/// See [`MOST_REQUEST_ID`]. An S3 key can be 1024 bytes; the head of it says which object.
pub const MOST_RESOURCE: usize = 256;
/// The most text one record holds.
pub const MOST_TEXT: usize =
    MOST_REQUEST_ID + MOST_CALLER + MOST_REGION + MOST_CODE + MOST_RESOURCE;
/// One record in the ring, without its text.
pub const RECORD_BYTES: usize = std::mem::size_of::<Call>();
/// The most the ring can hold, in bytes, when every text of every record is as long as it may be.
pub const MOST_BYTES: usize = CAPACITY * (RECORD_BYTES + MOST_TEXT);

/// One finished call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// Its number: 1 for the first call of this process.
    pub seq: u64,
    /// When it was answered, in milliseconds since 1970.
    pub at_ms: u64,
    /// From its head being read to its answer being made, in microseconds (a call over 71 minutes says 71 minutes).
    pub took_us: u32,
    /// The HTTP status answered.
    pub status: u16,
    /// The service's name in a credential scope; empty when the request named no service of this build.
    pub signing_name: &'static str,
    /// The service's SDK id (`DynamoDB Streams`).
    pub sdk_id: &'static str,
    /// The operation; empty when the request was refused before one was found.
    pub action: &'static str,
    pub request_id: Box<str>,
    /// The access key id the request was signed with.
    pub caller: Box<str>,
    pub region: Box<str>,
    /// The error code answered; empty for a success.
    pub code: Box<str>,
    /// What was acted on, when the gateway knew; empty otherwise.
    pub resource: Box<str>,
}

impl Call {
    /// The bytes this record holds: its slot and its text.
    #[must_use]
    pub fn bytes(&self) -> usize {
        RECORD_BYTES
            + self.request_id.len()
            + self.caller.len()
            + self.region.len()
            + self.code.len()
            + self.resource.len()
    }
}

/// `text`, cut at the last character boundary at or before `most` bytes.
fn cut(text: &str, most: usize) -> Box<str> {
    if text.len() <= most {
        return text.into();
    }
    let mut end = most;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default().into()
}

/// What a reader that has everything up to `after` is given.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Page {
    /// The calls after the reader's, oldest first.
    pub calls: Vec<Call>,
    /// The number of the last call given, or the reader's own when there is none: what to ask after next.
    pub last: u64,
    /// How many calls after the reader's the ring no longer holds.
    pub missed: u64,
    /// Whether more calls after `last` are held (the page was cut at its limit).
    pub more: bool,
}

/// How full the ring is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fill {
    /// Calls held.
    pub held: usize,
    /// Calls recorded since the process started (the number of the newest).
    pub recorded: u64,
    /// The bytes the held calls take.
    pub bytes: usize,
}

#[derive(Debug, Default)]
struct Ring {
    calls: VecDeque<Call>,
    /// The number of the newest call ever recorded.
    newest: u64,
    bytes: usize,
}

/// The ring, and the number of its newest call for whoever waits for the next one.
#[derive(Debug)]
pub struct Recorder {
    ring: Mutex<Ring>,
    capacity: usize,
    newest: watch::Sender<u64>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

impl Recorder {
    /// An empty ring of [`CAPACITY`] calls.
    #[must_use]
    pub fn new() -> Self {
        Self::holding(CAPACITY)
    }

    /// An empty ring of `capacity` calls (a test makes it small).
    #[must_use]
    pub fn holding(capacity: usize) -> Self {
        Self {
            ring: Mutex::new(Ring::default()),
            capacity: capacity.max(1),
            newest: watch::Sender::new(0),
        }
    }

    /// The most calls it holds.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Records one call, as answered at `at_ms`; its number.
    pub fn record(&self, call: &Finished<'_>, at_ms: u64) -> u64 {
        let mut made = Call {
            seq: 0,
            at_ms,
            took_us: u32::try_from(call.took.as_micros()).unwrap_or(u32::MAX),
            status: call.status,
            signing_name: call.signing_name,
            sdk_id: call.sdk_id,
            action: call.action,
            request_id: cut(call.request_id, MOST_REQUEST_ID),
            caller: cut(call.access_key_id, MOST_CALLER),
            region: cut(call.region, MOST_REGION),
            code: cut(call.code, MOST_CODE),
            resource: cut(call.resource, MOST_RESOURCE),
        };
        let seq = {
            let mut ring = self.ring.lock().unwrap_or_else(PoisonError::into_inner);
            ring.newest += 1;
            made.seq = ring.newest;
            if ring.calls.len() >= self.capacity
                && let Some(old) = ring.calls.pop_front()
            {
                ring.bytes -= old.bytes();
            }
            ring.bytes += made.bytes();
            ring.calls.push_back(made);
            ring.newest
        };
        // With no reader this stores a number; with readers it wakes them. It never blocks.
        self.newest.send_replace(seq);
        seq
    }

    /// The calls numbered after `after`, oldest first, at most `limit` of them.
    #[must_use]
    pub fn after(&self, after: u64, limit: usize) -> Page {
        let ring = self.ring.lock().unwrap_or_else(PoisonError::into_inner);
        let oldest = ring.calls.front().map_or(ring.newest + 1, |c| c.seq);
        let missed = oldest.saturating_sub(after + 1);
        // Numbers are consecutive in the ring, so the first wanted call is at a known place.
        let skip = usize::try_from((after + 1).saturating_sub(oldest)).unwrap_or(usize::MAX);
        let calls: Vec<Call> = ring.calls.iter().skip(skip).take(limit).cloned().collect();
        let last = calls.last().map_or(after.max(oldest - 1), |c| c.seq);
        Page {
            more: last < ring.newest,
            calls,
            last,
            missed,
        }
    }

    /// The newest `limit` calls, oldest first.
    #[must_use]
    pub fn newest(&self, limit: usize) -> Page {
        let from = self
            .newest_seq()
            .saturating_sub(u64::try_from(limit).unwrap_or(u64::MAX));
        Page {
            missed: 0,
            ..self.after(from, limit)
        }
    }

    /// The number of the newest call recorded; 0 before the first.
    #[must_use]
    pub fn newest_seq(&self) -> u64 {
        *self.newest.borrow()
    }

    /// How full it is.
    #[must_use]
    pub fn fill(&self) -> Fill {
        let ring = self.ring.lock().unwrap_or_else(PoisonError::into_inner);
        Fill {
            held: ring.calls.len(),
            recorded: ring.newest,
            bytes: ring.bytes,
        }
    }

    /// Something to wait on for the next call: it changes when one is recorded.
    #[must_use]
    pub fn watch(&self) -> watch::Receiver<u64> {
        self.newest.subscribe()
    }
}

impl Observer for Recorder {
    fn finished(&self, call: &Finished<'_>) {
        let at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        self.record(call, at_ms);
    }
}

#[cfg(test)]
mod tests;
