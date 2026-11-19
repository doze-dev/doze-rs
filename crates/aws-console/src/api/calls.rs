//! Live: `GET /api/v1/calls` (a page of the record) and `GET /api/v1/tail/live` (the same, as it happens, over
//! `text/event-stream`).
//!
//! A cursor is `<run>.<number>`: the run is this process's (its start, in milliseconds), the number is a call's. A reader that
//! comes back with a cursor is given exactly what came after it; one that comes back with the cursor of another run (doze-aws
//! restarted) is told to drop what it holds (`reset`).
//!
//! The tail owns no clock while nothing happens: it sleeps on the record's own signal and is woken by a call. After it has sent
//! an event it waits [`GAP`] before it looks again, so a burst of calls is a few events of many rows and not an event a call.
//! The only timer of an open, quiet tail is the keep-alive comment every [`KEEP_ALIVE`]; with no tail open there is none.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream::{self, Stream, StreamExt};
use serde::Serialize;
use tokio::sync::{OwnedSemaphorePermit, watch};

use super::{busy, json, param};
use crate::model::{CallRecord, CallsPage, RingFill, TailReady};
use crate::ring::Page;
use crate::state::{Shared, State as Console};

/// The calls a page holds when none is asked for.
pub const DEFAULT_LIMIT: usize = 200;
/// The most calls one event of a tail carries.
pub const BATCH: usize = 256;
/// The least time between two events of one tail.
pub const GAP: Duration = Duration::from_millis(250);
/// The comment line an open tail sends when it has nothing to say.
pub const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// The cursor of call `seq` of this run.
#[must_use]
pub fn cursor(s: &Console, seq: u64) -> String {
    format!("{}.{seq}", s.epoch)
}

/// The call a cursor of this run names; `None` for the cursor of another run, or text that is no cursor.
#[must_use]
pub fn seq_of(s: &Console, cursor: &str) -> Option<u64> {
    let (run, seq) = cursor.split_once('.')?;
    (run.parse::<u64>().ok()? == s.epoch)
        .then(|| seq.parse().ok())
        .flatten()
}

fn fill(s: &Console) -> RingFill {
    let f = s.recorder.fill();
    RingFill {
        capacity: u32::try_from(s.recorder.capacity()).unwrap_or(u32::MAX),
        held: u32::try_from(f.held).unwrap_or(u32::MAX),
        recorded: f.recorded,
        bytes: u64::try_from(f.bytes).unwrap_or(u64::MAX),
    }
}

/// A page of the record as the API sends it.
fn view(s: &Console, page: &Page, reset: bool) -> CallsPage {
    CallsPage {
        calls: page
            .calls
            .iter()
            .map(|c| CallRecord::new(c, s.facts.service_named(c.signing_name)))
            .collect(),
        cursor: cursor(s, page.last),
        reset,
        missed: page.missed,
        more: page.more,
        ring: fill(s),
    }
}

/// `GET /api/v1/calls?since=<cursor>&limit=<n>`: the calls after `since`, oldest first; with no `since`, the newest.
pub async fn calls(State(s): State<Shared>, RawQuery(query): RawQuery) -> Response {
    let query = query.as_deref();
    let limit = param(query, "limit")
        .and_then(|l| l.parse::<usize>().ok())
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, s.recorder.capacity());
    let asked = param(query, "since").map(|c| seq_of(&s, c));
    let (page, reset) = match asked {
        None => (s.recorder.newest(limit), false),
        Some(Some(seq)) if seq <= s.recorder.newest_seq() => (s.recorder.after(seq, limit), false),
        // Another run's cursor, or one ahead of this run.
        Some(_) => (s.recorder.newest(limit), true),
    };
    json(StatusCode::OK, &view(&s, &page, reset))
}

fn event<T: Serialize>(name: &str, value: &T, id: &str) -> Event {
    Event::default()
        .event(name)
        .id(id)
        .data(serde_json::to_string(value).unwrap_or_default())
}

/// One open tail.
struct Tail {
    s: Shared,
    woken: watch::Receiver<u64>,
    /// The last call sent.
    last: u64,
    /// An event was just sent: wait [`GAP`] before looking again.
    rest: bool,
    /// Held for as long as the stream is.
    _slot: OwnedSemaphorePermit,
}

impl Tail {
    /// The next event: what was recorded after the last one sent, when there is something. `None` when the console stops.
    async fn next(&mut self) -> Option<Event> {
        loop {
            if std::mem::take(&mut self.rest) {
                tokio::select! {
                    () = self.s.stop.cancelled() => return None,
                    () = tokio::time::sleep(GAP) => {}
                }
            }
            let page = self.s.recorder.after(self.last, BATCH);
            if !page.calls.is_empty() || page.missed > 0 {
                self.last = page.last;
                self.rest = true;
                let sent = view(&self.s, &page, false);
                return Some(event("calls", &sent, &sent.cursor));
            }
            // A call recorded between the look above and this wait has already changed the number: the wait returns at once.
            tokio::select! {
                () = self.s.stop.cancelled() => return None,
                changed = self.woken.changed() => changed.ok()?,
            }
        }
    }
}

/// The events of one tail: `ready` at once, then `calls` whenever some were recorded, until the console stops.
fn events(tail: Tail, ready: &TailReady) -> impl Stream<Item = Result<Event, Infallible>> + use<> {
    let first = stream::once(std::future::ready(Ok(event("ready", ready, &ready.cursor))));
    let rest = stream::unfold(tail, |mut tail| async move {
        let event = tail.next().await?;
        Some((Ok(event), tail))
    });
    first.chain(rest)
}

/// `GET /api/v1/tail/live?since=<cursor>`, `Last-Event-ID` winning over `since`. With neither, the tail starts from now.
pub async fn tail(
    State(s): State<Shared>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let asked = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .or_else(|| param(query.as_deref(), "since"));
    let now = s.recorder.newest_seq();
    let (last, reset) = match asked.map(|c| seq_of(&s, c)) {
        None => (now, false),
        Some(Some(seq)) if seq <= now => (seq, false),
        // Another run's cursor, or one ahead of this run: this run's calls, from the first held.
        Some(_) => (0, true),
    };
    let Ok(slot) = Arc::clone(&s.streams).try_acquire_owned() else {
        return busy(
            "The console has as many event streams open as it allows. Close a tail in another tab and switch this one on again.",
        );
    };
    let ready = TailReady {
        cursor: cursor(&s, last),
        reset,
    };
    let tail = Tail {
        woken: s.recorder.watch(),
        s,
        last,
        rest: false,
        _slot: slot,
    };
    Sse::new(events(tail, &ready))
        .keep_alive(KeepAlive::new().interval(KEEP_ALIVE).text("keep-alive"))
        .into_response()
}
