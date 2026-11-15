//! Acquiring records for a member from what a fetch read (Kafka's `SharePartition.acquire` and the methods it calls).
//!
//! Whole batches are acquired: a client is told the ranges it holds and works out the records itself, acknowledging as a
//! gap any offset it has no record for (a control record, one compacted away). A record that has been delivered often is
//! delivered with fewer others, and one on its last delivery alone, so that one bad record does not use up the deliveries
//! of a whole batch.

mod archive;
mod subset;

use std::cmp::min;

use super::{Count, Holder, InFlight, Limits, Rec, RecordState, SharePartition, Track};

/// One batch a fetch read, as acquisition needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) base: i64,
    pub(crate) last: i64,
    /// The producer, if the batch has one.
    pub(crate) producer: Option<i64>,
    pub(crate) transactional: bool,
    /// A control batch whose marker is an abort.
    pub(crate) abort_marker: bool,
}

/// An aborted transaction a `read_committed` fetch is told of: its producer and first offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Aborted {
    pub(crate) producer: i64,
    pub(crate) first: i64,
}

/// What a member asks to acquire.
#[derive(Debug, Clone)]
pub(crate) struct AcquireAsk {
    pub(crate) member: Holder,
    /// ShareFetch v2's record-limit mode: never more than `max_records`, splitting a batch if need be.
    pub(crate) record_limit: bool,
    /// The records per acquired range the client would like.
    pub(crate) batch_size: i32,
    /// The most records to acquire (soft, in batch-optimized mode: whole batches).
    pub(crate) max_records: i64,
    /// Aborted transactions' records are archived, not acquired (`read_committed`).
    pub(crate) read_committed: bool,
}

/// One acquired range, as the response tells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AcquiredRange {
    pub(crate) first: i64,
    pub(crate) last: i64,
    pub(crate) delivery_count: i16,
}

/// What an acquisition took.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Acquired {
    pub(crate) ranges: Vec<AcquiredRange>,
    /// How many offsets it took (what counts against the request's record limit).
    pub(crate) count: i64,
}

/// Kafka's `lastOffsetFromBatchWithRequestOffset`: the last offset of the batch holding `offset`, else `offset`.
fn last_of_batch_holding(spans: &[Span], offset: i64) -> i64 {
    spans
        .iter()
        .take_while(|s| offset >= s.base)
        .last()
        .filter(|s| offset <= s.last)
        .map_or(offset, |s| s.last)
}

/// What a fetch read, as acquisition needs it: the offset it read from, its batches, and (`read_committed`) the aborted
/// transactions among them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Fetched<'a> {
    pub(crate) from: i64,
    pub(crate) spans: &'a [Span],
    pub(crate) aborted: &'a [Aborted],
}

/// Who acquires, until when, under which limits: what every step of one acquisition shares.
struct Taker<'a> {
    ask: &'a AcquireAsk,
    spans: &'a [Span],
    /// When the locks taken now run out.
    until: i64,
    limits: &'a Limits,
}

/// What one acquisition may take: from the first batch read to the last offset it may acquire, and how many.
#[derive(Debug, Clone, Copy)]
struct Window {
    first: i64,
    last: i64,
    most: i64,
}

/// What one batch in flight gave: records, or records and a stop (Kafka's throttling ends the acquisition).
enum Took {
    Records(i64),
    Throttled(i64),
}

impl SharePartition {
    /// Acquires for `ask.member` what can be acquired of what a fetch read, at `now`.
    pub(crate) fn acquire(
        &mut self,
        ask: &AcquireAsk,
        read: &Fetched<'_>,
        now: i64,
        limits: &Limits,
    ) -> Acquired {
        let (Some(first_batch), Some(last_batch)) = (read.spans.first(), read.spans.last()) else {
            return Acquired::default();
        };
        if ask.max_records <= 0 || last_batch.last < self.start {
            return Acquired::default();
        }
        let (last, most) = self.last_and_most(read.from, ask.max_records, last_batch.last, limits);
        if most <= 0 {
            return Acquired::default();
        }
        let mut base = first_batch.base;
        self.archive_stale(read.from, base);
        match self.cached.range(..=base).next_back() {
            None if base < self.start => base = self.start,
            Some((&key, b)) if b.last >= base => base = key,
            _ => {}
        }
        let keys: Vec<i64> = self
            .cached
            .range(min(first_batch.base, base)..=last)
            .map(|(k, _)| *k)
            .collect();
        let t = Taker {
            ask,
            spans: read.spans,
            until: now + limits.lock_ms,
            limits,
        };
        if keys.is_empty() {
            let got = self.acquire_new(&t, (base, last), most);
            return self.filter_aborted(ask, read, got);
        }
        let window = Window {
            first: first_batch.base,
            last,
            most,
        };
        let got = self.acquire_cached(&t, &keys, base, window);
        match got.ranges.last() {
            Some(r) => {
                self.update_gap_start(r.last + 1);
                self.filter_aborted(ask, read, got)
            }
            None => got,
        }
    }

    /// The batches in flight from `base` (the keys of `cached` in the window), with what is new between them where the
    /// start had gaps, and after them what is new up to the window's end.
    fn acquire_cached(&mut self, t: &Taker<'_>, keys: &[i64], base: i64, w: Window) -> Acquired {
        let mut result = Vec::new();
        let mut acquired = 0;
        let mut most = w.most;
        let mut gap_from = base;
        for &key in keys {
            if acquired >= most {
                break;
            }
            if self.gap_active() {
                if gap_from < key {
                    let got = self.acquire_new(t, (gap_from, key - 1), most - acquired);
                    result.extend(got.ranges);
                    acquired += got.count;
                }
                gap_from = self.cached.get(&key).map_or(gap_from, |b| b.last + 1);
                if acquired >= most {
                    break;
                }
            }
            match self.take_batch(t, key, w, acquired, &mut result) {
                Took::Records(n) => acquired += n,
                Took::Throttled(n) => {
                    acquired += n;
                    most = 0;
                    break;
                }
            }
        }
        let last_cached = keys
            .last()
            .and_then(|k| self.cached.get(k))
            .map_or(i64::MAX, |b| b.last);
        if acquired < most && last_cached < w.last {
            let got = self.acquire_new(t, (last_cached + 1, w.last), most - acquired);
            result.extend(got.ranges);
            acquired += got.count;
        }
        Acquired {
            ranges: result,
            count: acquired,
        }
    }

    /// What of the batch at `key` is acquired: all of it when it is wholly in the window, available, kept whole and not
    /// to be throttled; else what of it is available, record by record. `acquired`: what this acquisition took before it.
    fn take_batch(
        &mut self,
        t: &Taker<'_>,
        key: i64,
        w: Window,
        acquired: i64,
        result: &mut Vec<AcquiredRange>,
    ) -> Took {
        let remaining = w.most - acquired;
        let Some(batch) = self.cached.get(&key) else {
            return Took::Records(0);
        };
        let full = key >= w.first && batch.last <= w.last;
        let record_limit_subset = t.ask.record_limit && batch.last - key + 1 > remaining;
        let throttle = self.should_throttle(key, w.first, w.last, t.limits);
        if throttle && acquired > 0 {
            return Took::Throttled(0);
        }
        if full && !batch.is_each() && !record_limit_subset && !throttle {
            return Took::Records(self.take_whole(t, key, result));
        }
        if batch
            .whole_rec()
            .is_some_and(|r| r.state != RecordState::Available)
        {
            return Took::Records(0);
        }
        if let Some(b) = self.cached.get_mut(&key) {
            b.explode(key);
        }
        let n = self.acquire_subset(t, remaining, (w.first, w.last), key, result);
        if throttle && n > 0 {
            Took::Throttled(n)
        } else {
            Took::Records(n)
        }
    }

    /// The batch at `key`, kept whole, acquired whole if it is available.
    fn take_whole(&mut self, t: &Taker<'_>, key: i64, result: &mut Vec<AcquiredRange>) -> i64 {
        let Some(b) = self.cached.get_mut(&key) else {
            return 0;
        };
        let Track::Whole(r) = &mut b.track else {
            return 0;
        };
        if r.state != RecordState::Available {
            return 0;
        }
        let Some(mut taken) = r.moved(
            RecordState::Acquired,
            Count::Increase,
            t.limits.max_delivery,
            Some(&t.ask.member),
        ) else {
            return 0;
        };
        taken.until = Some(t.until);
        *r = taken;
        result.push(AcquiredRange {
            first: key,
            last: b.last,
            delivery_count: r.count,
        });
        b.last - key + 1
    }

    /// Kafka's `lastOffsetAndMaxRecordsToAcquire`: how far, and how many, the in-flight limit lets this fetch acquire.
    fn last_and_most(
        &self,
        fetch_offset: i64,
        max_records: i64,
        last: i64,
        limits: &Limits,
    ) -> (i64, i64) {
        let most = min(max_records, limits.max_in_flight - self.in_flight());
        if most > 0 {
            return (last, most);
        }
        if fetch_offset <= self.end {
            // At the limit, re-acquiring between the records in flight.
            return (
                min(last, self.end),
                min(max_records, self.end - fetch_offset + 1),
            );
        }
        (last, most)
    }

    /// Kafka's `acquireNewBatchRecords`: records not yet in flight, from `first` to `last`.
    fn acquire_new(&mut self, t: &Taker<'_>, (first, last): (i64, i64), most: i64) -> Acquired {
        let first = if self.cached.is_empty() && self.end > first {
            self.end
        } else {
            first
        };
        let mut last_acquired = last;
        if most < last_acquired - first + 1 {
            last_acquired = last_of_batch_holding(t.spans, first + most - 1);
            if self.gap_active() && last_acquired > last {
                last_acquired = last;
            }
        }
        if last_acquired < first {
            return Acquired::default();
        }
        let ranges = self.create_batches(t, most, (first, last_acquired));
        if self
            .cached
            .first_key_value()
            .is_some_and(|(k, _)| *k == first)
        {
            self.start = first;
        }
        if last_acquired > self.end {
            self.end = last_acquired;
        }
        if t.ask.record_limit
            && let Some(r) = ranges.first()
        {
            last_acquired = r.last;
        }
        self.update_gap_start(last_acquired + 1);
        Acquired {
            ranges,
            count: last_acquired - first + 1,
        }
    }

    /// Kafka's `createBatches`: the new records as ranges of about the client's batch size, cut at batch boundaries; in
    /// record-limit mode one range, the batch split if it holds more than may be taken.
    fn create_batches(
        &mut self,
        t: &Taker<'_>,
        most: i64,
        (first, last): (i64, i64),
    ) -> Vec<AcquiredRange> {
        let (ask, until) = (t.ask, t.until);
        let batch_size = i64::from(ask.batch_size);
        let mut ranges = Vec::new();
        let mut current = first;
        if !ask.record_limit && batch_size > 0 && last - first + 1 > batch_size {
            for s in t.spans {
                if s.base > last {
                    break;
                }
                if s.base - current >= batch_size {
                    ranges.push(AcquiredRange {
                        first: current,
                        last: s.base - 1,
                        delivery_count: 1,
                    });
                    current = s.base;
                }
            }
        }
        ranges.push(AcquiredRange {
            first: current,
            last,
            delivery_count: 1,
        });
        if ask.record_limit
            && let Some(mut only) = ranges.first().copied()
            && only.last - only.first + 1 > most
        {
            let target = only.first + most - 1;
            let mut batch = InFlight::whole(only.last, Rec::acquired(&ask.member, until));
            batch.explode_up_to(only.first, target);
            self.find_next = true;
            self.cached.insert(only.first, batch);
            only.last = target;
            return vec![only];
        }
        for r in &ranges {
            self.cached.insert(
                r.first,
                InFlight::whole(r.last, Rec::acquired(&ask.member, until)),
            );
        }
        ranges
    }
}
