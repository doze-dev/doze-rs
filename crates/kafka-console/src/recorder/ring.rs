//! The ring: the newest rows, bounded by a count AND a byte budget, grown on demand (an idle ring holds nothing).
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
    pub fn record(&mut self, mut row: Row, fold: Fold) -> u64 {
        self.requests = self.requests.saturating_add(1);
        self.version = self.version.wrapping_add(1);
        let version = self.version;
        if fold != Fold::Never && row.fold != 0 {
            let window = if fold == Fold::Poll {
                FOLD_POLL_MS
            } else {
                FOLD_STREAM_MS
            };
            if let Some(id) = self.fold.get(&row.fold).copied()
                && let Some(prev) = self.get_mut(id)
                && row.first_ms.saturating_sub(prev.last_ms) <= window
            {
                let (before, after) = (prev.weight(), merge(prev, &row, version));
                self.bytes = self.bytes.saturating_sub(before).saturating_add(after);
                self.high_water = self.high_water.max(self.bytes);
                return id;
            }
        }
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

/// Folds `new` into `prev`; returns the weight of `prev` after.
fn merge(prev: &mut Row, new: &Row, version: u64) -> usize {
    prev.count = prev.count.saturating_add(new.count);
    prev.last_ms = new.last_ms.max(prev.last_ms);
    prev.touched = version;
    prev.correlation = new.correlation;
    prev.request_bytes = prev.request_bytes.saturating_add(new.request_bytes);
    prev.response_bytes = prev.response_bytes.saturating_add(new.response_bytes);
    prev.facts.records = prev.facts.records.saturating_add(new.facts.records);
    prev.facts.batches = prev.facts.batches.saturating_add(new.facts.batches);
    prev.facts.bytes = prev.facts.bytes.saturating_add(new.facts.bytes);
    prev.facts.from_offset = new.facts.from_offset.or(prev.facts.from_offset);
    for t in &new.tooks {
        if prev.tooks.len() >= SAMPLES {
            prev.tooks.remove(0);
        }
        prev.tooks.push(*t);
    }
    prev.weight()
}

#[cfg(test)]
mod tests;
