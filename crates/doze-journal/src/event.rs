//! One line of the journal: the event, and the parser for a line as the Go daemon wrote it.

use crate::gojson::{self, is};
use serde_json::value::RawValue;
use std::time::Duration;
use time::OffsetDateTime;

/// Why something happened.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cause {
    /// One of the `BY_*` kinds, or one a newer doze added; empty when none was recorded.
    pub kind: String,
    /// Who: the dependant for [`crate::BY_DEPENDENCY`], the path for [`crate::BY_WATCH`].
    pub by: String,
}

/// One event: a wake, sleep, crash or the like, with its cause and how long it took.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Event {
    /// The sequence number, unique across daemon restarts. Never 0 for an event read from disk.
    pub seq: u64,
    /// When it happened; `None` only for a line that carried no time.
    pub time: Option<OffsetDateTime>,
    /// One of the kinds ([`crate::WAKE`] and the rest), or one a newer doze added.
    pub kind: String,
    /// The instance it happened to.
    pub instance: String,
    /// Why it happened.
    pub cause: Cause,
    /// How long it took, in nanoseconds (Go's `time.Duration`); 0 when not recorded.
    pub took_ns: i64,
    /// What the kind needs said: how a crash exited, why a start failed, the step's name, "on" or "off" for a pin.
    pub detail: String,
}

impl Event {
    /// How long it took, if it was recorded and is not negative.
    pub fn took(&self) -> Option<Duration> {
        u64::try_from(self.took_ns)
            .ok()
            .filter(|&n| n > 0)
            .map(Duration::from_nanos)
    }
}

/// Parses one line (without its newline) as the Go daemon reads it back: `None` for a line that does not decode (a
/// write cut short by a crash) or that has no sequence number.
// fuzz:parser fuzz_line
pub fn parse_line(line: &[u8]) -> Option<Event> {
    let text = String::from_utf8_lossy(line);
    let text = gojson::mend_surrogates(&text);
    let mut e = Event::default();
    decode(&text, &mut e).ok()?;
    (e.seq != 0).then_some(e)
}

fn decode(text: &str, e: &mut Event) -> serde_json::Result<()> {
    for (key, raw) in gojson::object(text)?.unwrap_or_default() {
        field(&key, raw, e)?;
    }
    Ok(())
}

fn field(key: &str, raw: &RawValue, e: &mut Event) -> serde_json::Result<()> {
    if is(key, "seq") {
        set(&mut e.seq, gojson::value(raw)?);
    } else if is(key, "time") {
        if let Some(t) = gojson::time(raw)? {
            e.time = Some(t);
        }
    } else if is(key, "kind") {
        set(&mut e.kind, gojson::value(raw)?);
    } else if is(key, "instance") {
        set(&mut e.instance, gojson::value(raw)?);
    } else if is(key, "cause") {
        for (k, v) in gojson::object(raw.get())?.unwrap_or_default() {
            if is(&k, "kind") {
                set(&mut e.cause.kind, gojson::value(v)?);
            } else if is(&k, "by") {
                set(&mut e.cause.by, gojson::value(v)?);
            }
        }
    } else if is(key, "took") {
        set(&mut e.took_ns, gojson::value(raw)?);
    } else if is(key, "detail") {
        set(&mut e.detail, gojson::value(raw)?);
    }
    Ok(())
}

fn set<T>(slot: &mut T, v: Option<T>) {
    if let Some(v) = v {
        *slot = v;
    }
}
