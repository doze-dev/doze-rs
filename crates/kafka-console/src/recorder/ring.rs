//! The ring: the newest rows, bounded by a count AND a byte budget, grown on demand. A broker that has never been asked
//! anything holds nothing here; once it has, the rows stay (up to the bounds, about 0.4 to 0.6 MB at 2,000 rows and never more
//! than 1 MiB) however long it then sits idle, because they are the history the Live page shows ("reaches back to"), and a
//! developer back from lunch wants to see what happened before it. Rows are not aged out by time: only newer rows push them out.
//!
//! Folding lives here: a row that is the same kind of request as a recent one (same connection, API, version, target and result)
//! is added to it, so heartbeats and idle fetches do not push the interesting rows out.

use std::collections::{HashMap, VecDeque};

use super::row::{Row, SAMPLES};

/// The most rows kept.
pub const ROWS: usize = 2000;
/// The most bytes the rows may hold, by [`Row::weight`].
pub const BUDGET: usize = 1 << 20;
/// How long ago a poll may have been for the next one to fold into it.
pub const FOLD_POLL_MS: i64 = 10 * 60 * 1000;
/// How soon after the last one a request that is not a poll folds into it: only a stream of them does.
pub const FOLD_STREAM_MS: i64 = 2000;

/// How a row may fold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fold {
    Never,
    /// Repeats within ten minutes (heartbeats, polls that found nothing).
    Poll,
    /// Repeats within two seconds (a producer sending in a loop).
    Stream,
}

/// The rows.
#[derive(Debug, Default)]
pub struct Ring {
    rows: VecDeque<Row>,
    bytes: usize,
    next_id: u64,
    /// Bumped by every row added and every fold: what `since` counts from.
    version: u64,
    /// Requests recorded, folded or not.
    requests: u64,
    /// Fold key to the id of the row it folds into.
    fold: HashMap<u64, u64>,
    /// The most bytes the rows ever held (for the tests and the report).
    high_water: usize,
}

impl Ring {
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn requests(&self) -> u64 {
        self.requests
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn high_water(&self) -> usize {
        self.high_water
    }

    pub fn newest_id(&self) -> u64 {
        self.rows.back().map_or(0, |r| r.id)
    }

    /// The rows, oldest first.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Row> {
        self.rows.iter()
    }

    /// The row with `id`, if it is still kept.
    pub fn get(&self, id: u64) -> Option<&Row> {
        let first = self.rows.front()?.id;
        let at = usize::try_from(id.checked_sub(first)?).ok()?;
        self.rows.get(at).filter(|r| r.id == id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut Row> {
        let first = self.rows.front()?.id;
        let at = usize::try_from(id.checked_sub(first)?).ok()?;
        self.rows.get_mut(at).filter(|r| r.id == id)
    }

    /// Adds `row`, folded into a recent one if its kind allows. Returns the id of the row it became part of.
    pub fn record(&mut self, row: Row, fold: Fold) -> u64 {
        if let Some(id) = self.fold_in(row.fold, fold, &Repeat::of(&row)) {
            return id;
        }
        self.push(row, fold)
    }

    /// Folds a repeat into the recent row that has fold key `key`, if its kind allows and that row is recent enough. Returns
    /// the id of the row it became part of, or `None` (and nothing changed) when it must be a row of its own: the recorder
    /// asks this before it builds a row, so a repeat costs no row at all.
    pub fn fold_in(&mut self, key: u64, fold: Fold, repeat: &Repeat<'_>) -> Option<u64> {
        if fold == Fold::Never || key == 0 {
            return None;
        }
        let window = if fold == Fold::Poll {
            FOLD_POLL_MS
        } else {
            FOLD_STREAM_MS
        };
        let id = self.fold.get(&key).copied()?;
        let version = self.version.wrapping_add(1);
        let prev = self.get_mut(id)?;
        if repeat.at_ms.saturating_sub(prev.last_ms) > window {
            return None;
        }
        let (before, after) = (prev.weight(), merge(prev, repeat, version));
        self.requests = self.requests.saturating_add(1);
        self.version = version;
        self.bytes = self.bytes.saturating_sub(before).saturating_add(after);
        self.high_water = self.high_water.max(self.bytes);
        Some(id)
    }

    /// Adds `row` as a row of its own (after [`Ring::fold_in`] said it does not fold). Returns its id.
    pub fn push(&mut self, mut row: Row, fold: Fold) -> u64 {
        self.requests = self.requests.saturating_add(1);
        self.version = self.version.wrapping_add(1);
        let version = self.version;
        // The tape's order is the order of its times: a clock that stepped back never puts a row below an older one.
        if let Some(newest) = self.rows.back() {
            row.first_ms = row.first_ms.max(newest.first_ms);
            row.last_ms = row.last_ms.max(row.first_ms);
        }
        row.id = self.next_id;
        row.touched = version;
        self.next_id += 1;
        if row.fold != 0 && fold != Fold::Never {
            self.fold.insert(row.fold, row.id);
        }
        let id = row.id;
        self.bytes = self.bytes.saturating_add(row.weight());
        self.rows.push_back(row);
        self.evict();
        self.high_water = self.high_water.max(self.bytes);
        id
    }

    fn evict(&mut self) {
        while self.rows.len() > ROWS || (self.bytes > BUDGET && self.rows.len() > 1) {
            let Some(old) = self.rows.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(old.weight());
            if self.fold.get(&old.fold) == Some(&old.id) {
                self.fold.remove(&old.fold);
            }
        }
        if self.rows.is_empty() {
            // Idle means nothing held: give the memory back.
            self.rows = VecDeque::new();
            self.fold = HashMap::new();
            self.bytes = 0;
        }
    }
}

/// What a repeat adds to the row it folds into: its counts and times, and nothing it would need a row of its own to hold.
#[derive(Debug, Clone, Copy)]
pub struct Repeat<'a> {
    /// When it was answered (it folds only if the row's newest request was close enough before).
    pub at_ms: i64,
    /// When its newest request was answered: the same as `at_ms` for one request.
    pub last_ms: i64,
    pub count: u32,
    pub correlation: i32,
    pub request_bytes: u32,
    pub response_bytes: u32,
    pub records: u32,
    pub batches: u32,
    pub bytes: u64,
    pub from_offset: Option<i64>,
    /// How long it took, in microseconds; the newest last.
    pub tooks: &'a [u32],
}

impl<'a> Repeat<'a> {
    /// What `row` would add, were it folded.
    #[must_use]
    pub fn of(row: &'a Row) -> Self {
        Self {
            at_ms: row.first_ms,
            last_ms: row.last_ms,
            count: row.count,
            correlation: row.correlation,
            request_bytes: row.request_bytes,
            response_bytes: row.response_bytes,
            records: row.facts.records,
            batches: row.facts.batches,
            bytes: row.facts.bytes,
            from_offset: row.facts.from_offset,
            tooks: &row.tooks,
        }
    }
}

/// Folds `new` into `prev`; returns the weight of `prev` after.
fn merge(prev: &mut Row, new: &Repeat<'_>, version: u64) -> usize {
    prev.count = prev.count.saturating_add(new.count);
    prev.last_ms = new.last_ms.max(prev.last_ms);
    prev.touched = version;
    prev.correlation = new.correlation;
    prev.request_bytes = prev.request_bytes.saturating_add(new.request_bytes);
    prev.response_bytes = prev.response_bytes.saturating_add(new.response_bytes);
    prev.facts.records = prev.facts.records.saturating_add(new.records);
    prev.facts.batches = prev.facts.batches.saturating_add(new.batches);
    prev.facts.bytes = prev.facts.bytes.saturating_add(new.bytes);
    prev.facts.from_offset = new.from_offset.or(prev.facts.from_offset);
    for t in new.tooks {
        if prev.tooks.len() >= SAMPLES {
            prev.tooks.remove(0);
        }
        prev.tooks.push(*t);
    }
    prev.weight()
}

#[cfg(test)]
mod tests;
