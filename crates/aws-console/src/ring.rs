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
//!
//! The console's own calls (caller [`CONSOLE`](crate::resources::bus::CONSOLE): every page read is several) are let go first: once they are more than
//! [`CONSOLE_SHARE`] of a full ring, the oldest of them leaves instead of the oldest call, so browsing the console never pushes
//! an app's calls out of Live. Numbers then have gaps; a reader is given what is held after its number all the same.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use aws_gateway::{Finished, Observer};
use aws_peers::summary::Summary;
use aws_peers::{BusCall, CallWatcher};
use tokio::sync::watch;

use call::cut;
pub use call::{Call, Extra, Lab, LabDid, Via};
pub use fan::Publishing;
pub use flows::{FLOWS, Flow};

mod call;
mod fan;
mod flows;

/// The most calls held.
pub const CAPACITY: usize = 1024;
/// The part of a full ring the console's own calls may hold before they, not an app's, are let go: a quarter.
pub const CONSOLE_SHARE: usize = 4;
/// The most bytes of each text of a record that are kept.
pub const MOST_REQUEST_ID: usize = 64;
/// See [`MOST_REQUEST_ID`].
pub const MOST_CALLER: usize = 128;
/// See [`MOST_REQUEST_ID`].
pub const MOST_REGION: usize = 32;
/// See [`MOST_REQUEST_ID`].
pub const MOST_CODE: usize = 96;
/// See [`MOST_REQUEST_ID`]: the words of an error answer.
pub const MOST_MESSAGE: usize = 120;
/// See [`MOST_REQUEST_ID`]: `Unhandled`, `Handled`.
pub const MOST_FUNCTION_ERROR: usize = 16;
/// See [`MOST_REQUEST_ID`]. An S3 key can be 1024 bytes; the head of it says which object.
pub const MOST_RESOURCE: usize = 256;
/// See [`MOST_REQUEST_ID`]: each of the three texts of a call between services ([`Via`]).
pub const MOST_VIA: usize = 64;
/// The most text a request from the wire holds.
pub const MOST_WIRE_TEXT: usize = MOST_REQUEST_ID
    + MOST_CALLER
    + MOST_REGION
    + MOST_CODE
    + MOST_RESOURCE
    + crate::agent::MOST
    + MOST_MESSAGE
    + MOST_FUNCTION_ERROR
    + std::mem::size_of::<Extra>();
/// The most a call between services holds beside its slot: it has no request id of its own, and a [`Via`].
pub const MOST_BUS_TEXT: usize = MOST_CALLER
    + MOST_REGION
    + MOST_CODE
    + MOST_RESOURCE
    + std::mem::size_of::<Via>()
    + 3 * MOST_VIA;
/// The most one record holds beside its slot: the larger of the two.
pub const MOST_TEXT: usize = if MOST_WIRE_TEXT > MOST_BUS_TEXT {
    MOST_WIRE_TEXT
} else {
    MOST_BUS_TEXT
};
/// One record in the ring, without its text.
pub const RECORD_BYTES: usize = std::mem::size_of::<Call>();
/// The most the ring can hold, in bytes, when every text of every record is as long as it may be.
pub const MOST_BYTES: usize = CAPACITY * (RECORD_BYTES + MOST_TEXT);

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
    /// How many of `calls` are the console's own.
    console: usize,
    flows: flows::Flows,
    /// The number of the newest call ever recorded.
    newest: u64,
    bytes: usize,
}

/// Finds the request an access key, signed for a region, has in flight ([`aws_gateway::WouldDeny::request_of`]).
pub type FindCause = std::sync::Arc<dyn Fn(&str, &str) -> Option<String> + Send + Sync>;

/// The ring, and the number of its newest call for whoever waits for the next one.
pub struct Recorder {
    ring: Mutex<Ring>,
    /// Who asks the gateway which request a caller has in flight ([`Recorder::find_causes_with`]).
    causes: std::sync::OnceLock<FindCause>,
    /// The publishes the console is making now ([`Recorder::publishing`]).
    publishing: Mutex<Vec<Box<str>>>,
    capacity: usize,
    newest: watch::Sender<u64>,
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recorder")
            .field("capacity", &self.capacity)
            .field("causes", &self.causes.get().is_some())
            .finish_non_exhaustive()
    }
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
            causes: std::sync::OnceLock::new(),
            publishing: Mutex::new(Vec::new()),
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
        self.push(Call {
            seq: 0,
            at_ms,
            took_us: u32::try_from(call.took.as_micros()).unwrap_or(u32::MAX),
            status: call.status,
            would_be_denied: call.would_be_denied,
            signing_name: call.signing_name,
            sdk_id: call.sdk_id,
            action: call.action,
            request_id: cut(call.request_id, MOST_REQUEST_ID),
            caller: cut(call.access_key_id, MOST_CALLER),
            region: cut(call.region, MOST_REGION),
            code: cut(call.code, MOST_CODE),
            member: call.member,
            resource: cut(call.resource, MOST_RESOURCE),
            via: None,
            sdk: crate::agent::label(call.user_agent).into(),
            extra: (!call.message.is_empty()
                || !call.function_error.is_empty()
                || !call.did.detail.is_empty()
                || call.lab.is_some())
            .then(|| {
                Box::new(Extra {
                    message: cut(
                        if call.message.is_empty() {
                            call.did.detail
                        } else {
                            call.message
                        },
                        MOST_MESSAGE,
                    ),
                    function_error: cut(call.function_error, MOST_FUNCTION_ERROR),
                    lab: call.lab.and_then(|l| {
                        Lab::new(
                            l.rule,
                            match l.done {
                                aws_gateway::Done::Delayed(d) => LabDid::Delayed(
                                    u32::try_from(d.as_millis()).unwrap_or(u32::MAX),
                                ),
                                aws_gateway::Done::Failed => LabDid::Failed,
                                aws_gateway::Done::Dropped => LabDid::Dropped,
                            },
                        )
                    }),
                })
            }),
            answer_bytes: call
                .answer_bytes
                .map_or(u16::MAX, |n| u16::try_from(n).unwrap_or(u16::MAX - 1)),
            summary: self.deliveries(call),
        })
    }

    /// Sets how the cause of a call between services that carries none is found: the request its caller (access key, region)
    /// has in flight, when it has exactly one (the gateway knows). Set once; a second is ignored.
    pub fn find_causes_with(&self, find: FindCause) {
        drop(self.causes.set(find));
    }

    /// Records one call between services, as answered at `at_ms`; its number.
    pub fn record_bus(&self, call: &BusCall<'_>, at_ms: u64) -> u64 {
        let cause = call.cause;
        let text = |t: Option<&str>| cut(t.unwrap_or_default(), MOST_VIA);
        // A publish the console made says how many deliveries it led to; the request it ran under is the console's own, which nothing
        // answers, so the call does not name it as its cause.
        let published = self.console_publish(call);
        // A call made on a task the request's own does not reach (a service's actor) carries no cause: the request in flight for
        // its caller is. One a rule, mapping or schedule made is that thing's, whatever its caller has in flight.
        let found = if cause.is_none_or(|c| c.request.is_none() && c.name.is_none()) {
            self.console_publish_of(call).or_else(|| {
                self.causes
                    .get()
                    .and_then(|find| find(call.access_key_id, call.region))
            })
        } else {
            None
        };
        self.push(Call {
            seq: 0,
            at_ms,
            took_us: u32::try_from(call.took.as_micros()).unwrap_or(u32::MAX),
            status: call.status,
            would_be_denied: false,
            signing_name: call.signing_name,
            sdk_id: call.sdk_id,
            action: call.operation,
            request_id: Box::default(),
            caller: cut(call.access_key_id, MOST_CALLER),
            region: cut(call.region, MOST_REGION),
            code: cut(call.code, MOST_CODE),
            member: call.member,
            resource: cut(call.resource, MOST_RESOURCE),
            via: Some(Box::new(Via {
                by: cut(call.acting, MOST_VIA),
                request: text(
                    cause
                        .and_then(|c| c.request.as_deref())
                        .filter(|_| published.is_none())
                        .or(found.as_deref()),
                ),
                name: text(cause.and_then(|c| c.name.as_deref())),
            })),
            sdk: Box::default(),
            extra: None,
            answer_bytes: u16::MAX,
            summary: published.unwrap_or(Summary::NONE),
        })
    }

    /// Puts `made` in the ring as the newest call, and counts it in its flow; its number.
    fn push(&self, mut made: Call) -> u64 {
        let seq = {
            let mut ring = self.ring.lock().unwrap_or_else(PoisonError::into_inner);
            ring.newest += 1;
            made.seq = ring.newest;
            if ring.calls.len() >= self.capacity {
                // The console's own first, past its share; else the oldest.
                let at = if ring.console > self.capacity / CONSOLE_SHARE {
                    ring.calls.iter().position(is_console).unwrap_or(0)
                } else {
                    0
                };
                if let Some(old) = ring.calls.remove(at) {
                    ring.bytes -= old.bytes();
                    ring.console -= usize::from(is_console(&old));
                }
            }
            ring.bytes += made.bytes();
            ring.console += usize::from(is_console(&made));
            ring.flows.count(&made);
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
        // Numbers rise through the ring (with gaps where the console's own calls were let go): the first wanted is found by halves.
        let skip = ring.calls.partition_point(|c| c.seq <= after);
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
        let from = {
            let ring = self.ring.lock().unwrap_or_else(PoisonError::into_inner);
            let n = ring.calls.len();
            ring.calls
                .get(n.saturating_sub(limit))
                .map_or(ring.newest, |c| c.seq.saturating_sub(1))
        };
        Page {
            missed: 0,
            ..self.after(from, limit)
        }
    }

    /// The newest `limit` calls that are not the console's own, oldest first. A page that asks what the apps did (who polls an
    /// activity, who reads a secret) reads a window of the record; the console's own page reads (several for each page) must not
    /// spend it, or after a few minutes of browsing the window holds nothing of the apps'. The cursor is the newest call's, as for
    /// a page that reached the end.
    #[must_use]
    pub fn newest_apps(&self, limit: usize) -> Page {
        let ring = self.ring.lock().unwrap_or_else(PoisonError::into_inner);
        let mut calls: Vec<Call> = ring
            .calls
            .iter()
            .rev()
            .filter(|c| !is_console(c))
            .take(limit)
            .cloned()
            .collect();
        calls.reverse();
        Page {
            more: false,
            calls,
            last: ring.newest,
            missed: 0,
        }
    }

    /// The newest `limit` calls a Lab rule touched, newest first: those of rule `rule`, or of any rule when it is `None`; and
    /// whether the ring holds more of them than that.
    #[must_use]
    pub fn touched(&self, rule: Option<u32>, limit: usize) -> (Vec<Call>, bool) {
        let ring = self.ring.lock().unwrap_or_else(PoisonError::into_inner);
        let mut found = ring.calls.iter().rev().filter(|c| {
            c.extra
                .as_ref()
                .and_then(|e| e.lab.as_ref())
                .is_some_and(|l| rule.is_none_or(|r| r == l.rule()))
        });
        let calls: Vec<Call> = found.by_ref().take(limit).cloned().collect();
        let more = found.next().is_some();
        (calls, more)
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

    /// The call numbered `seq`, while the ring holds it.
    #[must_use]
    pub fn get(&self, seq: u64) -> Option<Call> {
        let ring = self.ring.lock().unwrap_or_else(PoisonError::into_inner);
        let at = ring.calls.binary_search_by_key(&seq, |c| c.seq).ok()?;
        ring.calls.get(at).cloned()
    }

    /// Every flow, most recent first, and how many were let go to keep the most at [`FLOWS`].
    #[must_use]
    pub fn flows(&self) -> (Vec<Flow>, u64) {
        let ring = self.ring.lock().unwrap_or_else(PoisonError::into_inner);
        ring.flows.all()
    }

    /// Something to wait on for the next call: it changes when one is recorded.
    #[must_use]
    pub fn watch(&self) -> watch::Receiver<u64> {
        self.newest.subscribe()
    }
}

/// Whether `call` is one the console made for its own pages.
fn is_console(call: &Call) -> bool {
    &*call.caller == crate::resources::bus::CONSOLE
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

impl Observer for Recorder {
    fn finished(&self, call: &Finished<'_>) {
        self.record(call, now_ms());
    }
}

impl CallWatcher for Recorder {
    fn called(&self, call: &BusCall<'_>) {
        self.record_bus(call, now_ms());
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_lab;
