//! Acquiring records for a member from what a fetch read (Kafka's `SharePartition.acquire` and the methods it calls).
//!
//! Whole batches are acquired: a client is told the ranges it holds and works out the records itself, acknowledging as a
//! gap any offset it has no record for (a control record, one compacted away). A record that has been delivered often is
//! delivered with fewer others, and one on its last delivery alone, so that one bad record does not use up the deliveries
//! of a whole batch.

use std::cmp::{max, min};

use super::{Count, Holder, InFlight, Limits, Rec, RecordState, SharePartition, Track, offset_of};

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

/// Kafka's `throttleRecordsDeliveryLimit`: half the delivery limit, rounded up, and at least 2.
fn throttle_limit(max_delivery: i32) -> i32 {
    max(2, (max_delivery + 1) / 2)
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

/// Kafka's `accumulateAcquiredRecords`: neighbouring offsets with the same delivery count, as one range.
fn accumulate(result: &mut Vec<AcquiredRange>, each: Vec<AcquiredRange>) {
    let mut each = each.into_iter();
    let Some(mut run) = each.next() else {
        return;
    };
    for r in each {
        if r.first == run.last + 1 && r.delivery_count == run.delivery_count {
            run.last = r.last;
        } else {
            result.push(run);
            run = r;
        }
    }
    result.push(run);
}

impl SharePartition {
    /// Acquires for `ask.member` what can be acquired of `spans`, read from `fetch_offset`, at `now`.
    pub(crate) fn acquire(
        &mut self,
        ask: &AcquireAsk,
        fetch_offset: i64,
        spans: &[Span],
        aborted: &[Aborted],
        now: i64,
        limits: &Limits,
    ) -> Acquired {
        let (Some(first_batch), Some(last_batch)) = (spans.first(), spans.last()) else {
            return Acquired::default();
        };
        if ask.max_records <= 0 || last_batch.last < self.start {
            return Acquired::default();
        }
        let (last_to_acquire, mut max_to_acquire) =
            self.last_and_most(fetch_offset, ask.max_records, last_batch.last, limits);
        if max_to_acquire <= 0 {
            return Acquired::default();
        }
        let mut base = first_batch.base;
        self.archive_stale(fetch_offset, base);
        match self.cached.range(..=base).next_back() {
            None if base < self.start => base = self.start,
            Some((&key, b)) if b.last >= base => base = key,
            _ => {}
        }
        let keys: Vec<i64> = self
            .cached
            .range(min(first_batch.base, base)..=last_to_acquire)
            .map(|(k, _)| *k)
            .collect();
        let Some(&last_key) = keys.last() else {
            let got = self.acquire_new(
                ask,
                spans,
                (base, last_to_acquire),
                max_to_acquire,
                now,
                limits,
            );
            return self.filter_aborted(ask, spans, aborted, got);
        };
        let mut result = Vec::new();
        let mut acquired = 0;
        let mut gap_from = base;
        for key in keys {
            if acquired >= max_to_acquire {
                break;
            }
            if self.gap_active() {
                if gap_from < key {
                    let got = self.acquire_new(
                        ask,
                        spans,
                        (gap_from, key - 1),
                        max_to_acquire - acquired,
                        now,
                        limits,
                    );
                    result.extend(got.ranges);
                    acquired += got.count;
                }
                gap_from = self.cached.get(&key).map_or(gap_from, |b| b.last + 1);
                if acquired >= max_to_acquire {
                    break;
                }
            }
            let Some(batch) = self.cached.get(&key) else {
                continue;
            };
            let full = key >= first_batch.base && batch.last <= last_to_acquire;
            let remaining = max_to_acquire - acquired;
            let record_limit_subset = ask.record_limit && batch.last - key + 1 > remaining;
            let throttle = self.should_throttle(key, first_batch.base, last_to_acquire, limits);
            if throttle && acquired > 0 {
                max_to_acquire = 0;
                break;
            }
            if !full || batch.is_each() || record_limit_subset || throttle {
                if batch
                    .whole_rec()
                    .is_some_and(|r| r.state != RecordState::Available)
                {
                    continue;
                }
                if let Some(b) = self.cached.get_mut(&key) {
                    b.explode(key);
                }
                let n = self.acquire_subset(
                    ask,
                    remaining,
                    (first_batch.base, last_to_acquire),
                    key,
                    &mut result,
                    now,
                    limits,
                );
                acquired += n;
                if throttle && n > 0 {
                    max_to_acquire = 0;
                    break;
                }
                continue;
            }
            let Some(b) = self.cached.get_mut(&key) else {
                continue;
            };
            let Track::Whole(r) = &mut b.track else {
                continue;
            };
            if r.state != RecordState::Available {
                continue;
            }
            let Some(mut taken) = r.moved(
                RecordState::Acquired,
                Count::Increase,
                limits.max_delivery,
                Some(&ask.member),
            ) else {
                continue;
            };
            taken.until = Some(now + limits.lock_ms);
            *r = taken;
            result.push(AcquiredRange {
                first: key,
                last: b.last,
                delivery_count: r.count,
            });
            acquired += b.last - key + 1;
        }
        let last_cached = self.cached.get(&last_key).map_or(i64::MAX, |b| b.last);
        if acquired < max_to_acquire && last_cached < last_to_acquire {
            let got = self.acquire_new(
                ask,
                spans,
                (last_cached + 1, last_to_acquire),
                max_to_acquire - acquired,
                now,
                limits,
            );
            result.extend(got.ranges);
            acquired += got.count;
        }
        let got = Acquired {
            ranges: result,
            count: acquired,
        };
        match got.ranges.last() {
            Some(last) => {
                self.update_gap_start(last.last + 1);
                self.filter_aborted(ask, spans, aborted, got)
            }
            None => got,
        }
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
    fn acquire_new(
        &mut self,
        ask: &AcquireAsk,
        spans: &[Span],
        (first, last): (i64, i64),
        most: i64,
        now: i64,
        limits: &Limits,
    ) -> Acquired {
        let first = if self.cached.is_empty() && self.end > first {
            self.end
        } else {
            first
        };
        let mut last_acquired = last;
        if most < last_acquired - first + 1 {
            last_acquired = last_of_batch_holding(spans, first + most - 1);
            if self.gap_active() && last_acquired > last {
                last_acquired = last;
            }
        }
        if last_acquired < first {
            return Acquired::default();
        }
        let ranges = self.create_batches(ask, spans, most, (first, last_acquired), now, limits);
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
        if ask.record_limit
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
        ask: &AcquireAsk,
        spans: &[Span],
        most: i64,
        (first, last): (i64, i64),
        now: i64,
        limits: &Limits,
    ) -> Vec<AcquiredRange> {
        let until = now + limits.lock_ms;
        let batch_size = i64::from(ask.batch_size);
        let mut ranges = Vec::new();
        let mut current = first;
        if !ask.record_limit && batch_size > 0 && last - first + 1 > batch_size {
            for s in spans {
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

    /// Kafka's `shouldThrottleRecordsDelivery`: the batch has an available record delivered at least half the limit.
    fn should_throttle(&self, key: i64, req_first: i64, req_last: i64, limits: &Limits) -> bool {
        let limit = throttle_limit(limits.max_delivery);
        let Some(b) = self.cached.get(&key) else {
            return false;
        };
        match &b.track {
            Track::Whole(r) => r.state == RecordState::Available && i32::from(r.count) >= limit,
            Track::Each(_) => {
                b.records(key)
                    .filter(|(o, r)| {
                        *o >= req_first && *o <= req_last && r.state == RecordState::Available
                    })
                    .map(|(_, r)| i32::from(r.count))
                    .max()
                    .unwrap_or(0)
                    >= limit
            }
        }
    }

    /// Kafka's `acquireSubsetBatchRecords`: the available records of a batch kept per offset, from `req_first` to `req_last`.
    #[expect(
        clippy::too_many_arguments,
        reason = "Kafka's method, with its inputs; the time and limits besides"
    )]
    fn acquire_subset(
        &mut self,
        ask: &AcquireAsk,
        most: i64,
        (req_first, req_last): (i64, i64),
        key: i64,
        result: &mut Vec<AcquiredRange>,
        now: i64,
        limits: &Limits,
    ) -> i64 {
        let max_delivery = limits.max_delivery;
        let limit = throttle_limit(max_delivery);
        let Some(b) = self.cached.get_mut(&key) else {
            return 0;
        };
        let Track::Each(records) = &mut b.track else {
            return 0;
        };
        let size = offset_of(records.len());
        let mut acquired = 0;
        let mut most_while_throttled: Option<i64> = None;
        let mut taken = Vec::new();
        for (i, r) in records.iter_mut().enumerate() {
            let offset = key + offset_of(i);
            if offset < req_first {
                continue;
            }
            if offset > req_last {
                break;
            }
            if r.state != RecordState::Available {
                continue;
            }
            let count = i32::from(r.count);
            // On its last delivery a record goes alone: one more with others would spend theirs if it is the bad one.
            if max_delivery > 2 && count == max_delivery - 1 && acquired > 0 {
                break;
            }
            if count >= limit && most_while_throttled.is_none() {
                let shift = u32::try_from(count - limit + 1).unwrap_or(u32::MAX);
                most_while_throttled = Some(max(1, size.checked_shr(shift).unwrap_or(0)));
            }
            let Some(mut next) = r.moved(
                RecordState::Acquired,
                Count::Increase,
                max_delivery,
                Some(&ask.member),
            ) else {
                continue;
            };
            next.until = Some(now + limits.lock_ms);
            *r = next;
            taken.push(AcquiredRange {
                first: offset,
                last: offset,
                delivery_count: r.count,
            });
            acquired += 1;
            if i32::from(r.count) == max_delivery && max_delivery > 2 {
                break;
            }
            if ask.record_limit && acquired == most {
                break;
            }
            if most_while_throttled == Some(acquired) {
                break;
            }
        }
        accumulate(result, taken);
        acquired
    }

    /// Kafka's `maybeArchiveStaleBatches`: batches from the fetch offset to the first batch read are gone from the log
    /// (compacted away), so what of them is available is archived, or the start could never pass them.
    fn archive_stale(&mut self, fetch_offset: i64, base: i64) {
        if self.cached.is_empty() || fetch_offset >= base {
            return;
        }
        let floor = match self.cached.range(..=fetch_offset).next_back() {
            Some((&k, b)) if b.last >= fetch_offset => k,
            _ => fetch_offset,
        };
        let keys: Vec<i64> = self.cached.range(floor..base).map(|(k, _)| *k).collect();
        if self.archive_records((fetch_offset, base), &keys, RecordState::Available) {
            self.find_next = true;
        }
    }

    /// Kafka's `archiveRecords`: in the batches `keys`, what is in state `from` between `first` and `end` (exclusive) is
    /// archived. `true`: something was.
    pub(super) fn archive_records(
        &mut self,
        (first, end): (i64, i64),
        keys: &[i64],
        from: RecordState,
    ) -> bool {
        let mut any = false;
        for &key in keys {
            if key >= end {
                break;
            }
            let Some(b) = self.cached.get_mut(&key) else {
                continue;
            };
            let full = key >= first && b.last <= end - 1;
            if full && let Track::Whole(r) = &mut b.track {
                if r.state == from {
                    r.archive();
                    any = true;
                }
                continue;
            }
            if b.whole_rec().is_some_and(|r| r.state != from) {
                continue;
            }
            b.explode(key);
            if let Track::Each(records) = &mut b.track {
                for (i, r) in records.iter_mut().enumerate() {
                    let offset = key + offset_of(i);
                    if offset >= first && offset < end && r.state == from {
                        r.archive();
                        any = true;
                    }
                }
            }
        }
        any
    }

    /// Kafka's `maybeFilterAbortedTransactionalAcquiredRecords`: under `read_committed`, what an aborted transaction wrote
    /// is archived as soon as it is acquired, and taken out of what the member is told it holds.
    fn filter_aborted(
        &mut self,
        ask: &AcquireAsk,
        spans: &[Span],
        aborted: &[Aborted],
        got: Acquired,
    ) -> Acquired {
        if !ask.read_committed || aborted.is_empty() {
            return got;
        }
        let doomed = aborted_batches(spans, aborted);
        for s in &doomed {
            let Some((&floor, _)) = self.cached.range(..=s.base).next_back() else {
                continue;
            };
            let keys: Vec<i64> = self.cached.range(floor..=s.last).map(|(k, _)| *k).collect();
            self.archive_records((s.base, s.last + 1), &keys, RecordState::Acquired);
        }
        let ranges = without(got.ranges, &doomed);
        let count = ranges.iter().map(|r| r.last - r.first + 1).sum();
        Acquired { ranges, count }
    }
}

/// Kafka's `fetchAbortedTransactionRecordBatches`: the transactional batches of a producer whose transaction was aborted,
/// up to its abort marker.
fn aborted_batches(spans: &[Span], aborted: &[Aborted]) -> Vec<Span> {
    let mut pending: Vec<Aborted> = aborted.to_vec();
    pending.sort_by_key(|a| std::cmp::Reverse(a.first));
    let mut producers = std::collections::HashSet::new();
    let mut out = Vec::new();
    for s in spans {
        let Some(producer) = s.producer else {
            continue;
        };
        while pending.last().is_some_and(|a| a.first <= s.last) {
            if let Some(a) = pending.pop() {
                producers.insert(a.producer);
            }
        }
        if s.abort_marker {
            producers.remove(&producer);
        } else if s.transactional && producers.contains(&producer) {
            out.push(*s);
        }
    }
    out
}

/// Kafka's `filterRecordBatchesFromAcquiredRecords`: the acquired ranges less the batches to archive.
fn without(ranges: Vec<AcquiredRange>, doomed: &[Span]) -> Vec<AcquiredRange> {
    let mut out = Vec::new();
    for mut r in ranges {
        let mut whole = true;
        for d in doomed {
            if r.last < d.base || r.first > d.last {
                continue;
            }
            if r.first < d.base {
                out.push(AcquiredRange {
                    last: d.base - 1,
                    ..r
                });
            }
            if r.last > d.last {
                r.first = d.last + 1;
            } else {
                whole = false;
                break;
            }
        }
        if whole {
            out.push(r);
        }
    }
    out
}
