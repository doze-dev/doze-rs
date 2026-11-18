//! What acquisition archives instead of acquiring: batches compacted away before the fetch offset, and (`read_committed`)
//! what an aborted transaction wrote.

use super::{Aborted, AcquireAsk, Acquired, AcquiredRange, Fetched, Span};
use crate::share::partition::{RecordState, SharePartition, Track, offset_of};

impl SharePartition {
    /// Kafka's `maybeArchiveStaleBatches`: batches from the fetch offset to the first batch read are gone from the log
    /// (compacted away), so what of them is available is archived, or the start could never pass them.
    pub(super) fn archive_stale(&mut self, fetch_offset: i64, base: i64) {
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
    pub(in crate::share::partition) fn archive_records(
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
            let full = key >= first && b.last < end;
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
    pub(super) fn filter_aborted(
        &mut self,
        ask: &AcquireAsk,
        read: &Fetched<'_>,
        got: Acquired,
    ) -> Acquired {
        if !ask.read_committed || read.aborted.is_empty() {
            return got;
        }
        let doomed = aborted_batches(read.spans, read.aborted);
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
