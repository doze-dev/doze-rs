//! One query's run over a snapshot of the event table: off the actor (on the blocking pool, at most [`WORKERS`] at once) when the
//! service runs on a runtime, in line when it is under test. The streams are read newest first and merged by time, so the engine
//! sees the region's events in the order a query with no `sort` answers in, and stops reading once the answer cannot change.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use doze_store::rows::RowSnapshot;

use super::ast::Query;
use super::record::Event;
use super::run::Engine;
use crate::model::{self, event_key};
use crate::queries::{QueryRun, Status};

/// Queries read at once; the others wait their turn as `Scheduled`.
pub(crate) const WORKERS: usize = 2;
/// Rows read from one stream at a time.
const CHUNK: usize = 256;

/// A stream a query reads.
#[derive(Clone, Debug)]
pub(crate) struct Source {
    pub(crate) no: u32,
    pub(crate) name: String,
    pub(crate) group: String,
    pub(crate) region: String,
}

/// Everything a run needs, owned, so it can cross to another thread.
#[derive(Debug)]
pub(crate) struct Job {
    pub(crate) run: Arc<QueryRun>,
    pub(crate) query: Query,
    pub(crate) snapshot: RowSnapshot,
    pub(crate) sources: Vec<Source>,
    pub(crate) account: String,
    /// The time range, in milliseconds, both ends included.
    pub(crate) from_ms: i64,
    pub(crate) to_ms: i64,
    pub(crate) cap: usize,
}

/// One stream's place in the read.
struct Cursor {
    /// Rows read and not yet given to the engine, oldest first (the newest is popped).
    buf: Vec<(String, Vec<u8>)>,
    /// The key below which the next chunk is read; `None` once the stream is read to the start of the range.
    below: Option<String>,
}

impl Job {
    /// Runs the query and leaves the answer in its run.
    pub(crate) fn execute(self) {
        let started = std::time::Instant::now();
        self.run.set_status(Status::Running);
        let mut engine = Engine::new(&self.query, self.cap);
        let (mut scanned, mut bytes) = (0u64, 0u64);
        let mut cursors: Vec<Cursor> = self
            .sources
            .iter()
            .map(|s| Cursor {
                buf: Vec::new(),
                below: Some(event_key(s.no, self.to_ms.saturating_add(1), 0)),
            })
            .collect();
        let mut heap = BinaryHeap::new();
        for i in 0..cursors.len() {
            self.refill(&mut cursors, i, &mut heap);
        }
        while let Some((Reverse(_), i)) = heap.pop().map(|(k, i)| (Reverse(k), i)) {
            if self.run.cancel.load(Ordering::Relaxed) || engine.done() {
                break;
            }
            let Some((key, value)) = cursors.get_mut(i).and_then(|c| c.buf.pop()) else {
                continue;
            };
            scanned += 1;
            bytes += value.len() as u64;
            self.feed(&mut engine, i, &key, &value);
            self.refill(&mut cursors, i, &mut heap);
        }
        let out = engine.finish();
        let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.run.finish(out, scanned, bytes, ms);
    }

    /// Puts stream `i`'s next row in the heap, reading a chunk when its buffer is empty.
    fn refill(&self, cursors: &mut [Cursor], i: usize, heap: &mut BinaryHeap<(String, usize)>) {
        let (Some(c), Some(s)) = (cursors.get_mut(i), self.sources.get(i)) else {
            return;
        };
        if c.buf.is_empty()
            && let Some(below) = c.below.take()
        {
            let from = event_key(s.no, self.from_ms.max(0), 0);
            let mut chunk = self
                .snapshot
                .scan(&from, &below, CHUNK, true)
                .unwrap_or_default();
            if chunk.len() == CHUNK {
                c.below = chunk.last().map(|(k, _)| k.clone());
            }
            chunk.reverse();
            c.buf = chunk;
        }
        if let Some((key, _)) = c.buf.last() {
            // The heap orders by the time and sequence in the key (after the stream number), newest on top.
            heap.push((key.get(8..).unwrap_or_default().to_owned(), i));
        }
    }

    fn feed(&self, engine: &mut Engine<'_>, i: usize, key: &str, value: &[u8]) {
        let (Some(s), Ok(e)) = (self.sources.get(i), model::Event::decode(value)) else {
            return;
        };
        let ts = key
            .get(8..24)
            .and_then(|h| i64::from_str_radix(h, 16).ok())
            .unwrap_or(0);
        let event = Event {
            ts,
            ingested: e.ingested_ms,
            message: &e.message,
            transformed: e.transformed.as_deref(),
            stream: &s.name,
            group: &s.group,
            account: &self.account,
            ptr: pointer(&s.region, &s.group, key),
            lambda: s.group.starts_with("/aws/lambda/"),
        };
        engine.push(&event);
    }
}

/// `@ptr`: the region, the group and the event's row key, base64-encoded (AWS's is its own opaque text).
pub(crate) fn pointer(region: &str, group: &str, key: &str) -> String {
    aws_core::base64::encode(format!("{region}\u{1}{group}\u{1}{key}").as_bytes())
}

/// What a pointer names: the region, the group and the row key.
pub(crate) fn unpoint(ptr: &str) -> Option<(String, String, String)> {
    let raw = aws_core::base64::decode(ptr)?;
    let text = String::from_utf8(raw).ok()?;
    let mut parts = text.splitn(3, '\u{1}');
    let (region, group, key) = (parts.next()?, parts.next()?, parts.next()?);
    let valid = key.len() == 40 && key.bytes().all(|b| b.is_ascii_hexdigit());
    valid.then(|| (region.to_owned(), group.to_owned(), key.to_owned()))
}
