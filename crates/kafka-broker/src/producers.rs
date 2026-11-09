//! What a partition remembers of its idempotent producers, so that a batch a producer sends again (it timed out
//! waiting for the answer) is recognised and not written twice.
//!
//! A producer stamps each batch with its id, its epoch and the sequence number of the batch's first record. Per
//! producer the partition keeps the epoch and the sequence ranges of the last five batches (Kafka's window of
//! in-flight batches for an idempotent producer) with the offset each was given. This is pure: [`Producers::check`]
//! judges a batch, and [`Producers::record`] remembers one that was written. Whoever owns the partition's writer
//! calls them one after the other, with the append between, so two retries cannot both be written.
//!
//! Sequence numbers are 31 bits and wrap to 0.

use std::collections::{HashMap, VecDeque};

/// How many recent batches are kept per producer.
const RECENT: usize = 5;
/// A producer silent for this long is forgotten (Kafka's `producer.id.expiration.ms`, one day).
const IDLE_MS: i64 = 24 * 60 * 60 * 1000;
/// The most producers one partition remembers, so that memory stays bounded however many write.
const MOST: usize = 50_000;
/// The idle are looked for after this many more producers have been remembered.
const SWEEP_EVERY: usize = 1_000;

/// A batch that was written: its sequence range, where it went and what its newest timestamp was as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Seen {
    first: i32,
    last: i32,
    base_offset: i64,
    max_timestamp: i64,
}

#[derive(Debug)]
struct Producer {
    epoch: i16,
    /// Ascending by sequence; the newest last.
    recent: VecDeque<Seen>,
    last_used_ms: i64,
    /// Only a marker of this epoch is known, found in the log when the partition was opened: the batches it ended were
    /// not read back (retention or DeleteRecords took them, or they are older than the tail that is read), so where the
    /// producer's sequences stand is not known and its next batch is its baseline.
    sequences_lost: bool,
}

/// What a batch is to be done with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// In order, or from a producer this partition knows nothing of: write it, then [`Producers::record`] it.
    Accept,
    /// Already written: answer with where it went and its timestamp as stored; write nothing.
    Duplicate { offset: i64, timestamp: i64 },
    /// A gap, an overlap with what was written, or a repeat of a batch too old to be remembered (Kafka tells these apart
    /// only by whether it still has the batch): OUT_OF_ORDER_SEQUENCE_NUMBER.
    OutOfOrder,
    /// An epoch older than the producer's: INVALID_PRODUCER_EPOCH.
    Fenced,
}

/// A batch that was written: whose it was, its sequence range, where it went and its newest timestamp as stored.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stored {
    pub(crate) id: i64,
    pub(crate) epoch: i16,
    pub(crate) first: i32,
    pub(crate) last: i32,
    pub(crate) base_offset: i64,
    pub(crate) max_timestamp: i64,
}

/// A partition's idempotent producers.
#[derive(Debug, Default)]
pub(crate) struct Producers {
    by_id: HashMap<i64, Producer>,
    swept_at: usize,
}

/// The sequence that follows `last`.
fn next(last: i32) -> i32 {
    if last == i32::MAX { 0 } else { last + 1 }
}

/// The sequence of the last record of a batch of `count` records whose first is `first`.
pub(crate) fn last_of(first: i32, count: u32) -> i32 {
    let last = i64::from(first) + i64::from(count) - 1;
    i32::try_from(last & i64::from(i32::MAX)).unwrap_or(0)
}

impl Producers {
    /// How a batch from `id` at `epoch`, whose records are numbered `first..=last`, is to be treated. Changes nothing.
    #[must_use]
    pub(crate) fn check(&self, id: i64, epoch: i16, first: i32, last: i32) -> Verdict {
        let Some(p) = self.by_id.get(&id) else {
            // A producer this partition has no memory of (new, or from before a restart): this batch is its baseline.
            return Verdict::Accept;
        };
        if epoch < p.epoch {
            return Verdict::Fenced;
        }
        if epoch > p.epoch {
            // A new epoch is a new session, and a session starts at sequence 0.
            return if first == 0 {
                Verdict::Accept
            } else {
                Verdict::OutOfOrder
            };
        }
        let Some(newest) = p.recent.back() else {
            // Only a marker is known at this epoch. If it raised the epoch, the sequences start at 0; if it was found in a
            // log whose batches are gone, they are not known, as for a producer never seen (Kafka's `checkSequence`: "accept
            // writes with any sequence number" of a producer whose records retention or DeleteRecords deleted).
            return if first == 0 || p.sequences_lost {
                Verdict::Accept
            } else {
                Verdict::OutOfOrder
            };
        };
        if first == next(newest.last) {
            return Verdict::Accept;
        }
        // Not the next batch. An exact repeat of one remembered is a retry: answer with where it went. Something older
        // than all that is remembered was written long ago. Anything else, a gap or an overlap, is out of order.
        if let Some(seen) = p.recent.iter().find(|s| s.first == first && s.last == last) {
            return Verdict::Duplicate {
                offset: seen.base_offset,
                timestamp: seen.max_timestamp,
            };
        }
        Verdict::OutOfOrder
    }

    /// Remembers a batch that was written at `now_ms`.
    pub(crate) fn record(&mut self, w: Stored, now_ms: i64) {
        let Stored {
            id,
            epoch,
            first,
            last,
            base_offset,
            max_timestamp,
        } = w;
        let p = match self.by_id.get_mut(&id) {
            Some(p) if p.epoch >= epoch => p,
            _ => self
                .by_id
                .entry(id)
                .insert_entry(Producer {
                    epoch,
                    recent: VecDeque::new(),
                    last_used_ms: now_ms,
                    sequences_lost: false,
                })
                .into_mut(),
        };
        p.recent.push_back(Seen {
            first,
            last,
            base_offset,
            max_timestamp,
        });
        while p.recent.len() > RECENT {
            p.recent.pop_front();
        }
        p.last_used_ms = now_ms;
        if self.by_id.len() >= self.swept_at + SWEEP_EVERY || self.by_id.len() > MOST {
            self.forget_idle(id, now_ms);
            self.swept_at = self.by_id.len();
        }
    }

    /// Drops the producers silent for a day and, if the partition is still over its ceiling with everyone recently
    /// active, the least recently used beyond it (a tenth of the ceiling at a time, so that this is rare). `keep` is the
    /// producer that has just written.
    fn forget_idle(&mut self, keep: i64, now_ms: i64) {
        let cutoff = now_ms.saturating_sub(IDLE_MS);
        self.by_id
            .retain(|id, p| *id == keep || p.last_used_ms >= cutoff);
        if let Some(over) = self.by_id.len().checked_sub(MOST).filter(|o| *o > 0) {
            let mut oldest: Vec<(i64, i64)> = self
                .by_id
                .iter()
                .filter(|(id, _)| **id != keep)
                .map(|(id, p)| (p.last_used_ms, *id))
                .collect();
            oldest.sort_unstable();
            for (_, id) in oldest.into_iter().take(over + MOST / 10) {
                self.by_id.remove(&id);
            }
        }
    }

    /// The producers remembered: (id, epoch, last sequence, last used in ms), in no order.
    pub(crate) fn snapshot(&self) -> Vec<(i64, i16, i32, i64)> {
        self.by_id
            .iter()
            .map(|(id, p)| {
                (
                    *id,
                    p.epoch,
                    p.recent.back().map_or(-1, |s| s.last),
                    p.last_used_ms,
                )
            })
            .collect()
    }

    /// A transaction marker for `id` was written at `epoch`: the producer is at least that epoch, and its sequences start
    /// again (Kafka's state is bumped by a marker, so a producer that has been fenced is refused here too).
    pub(crate) fn marked(&mut self, id: i64, epoch: i16, now_ms: i64) {
        self.mark(id, epoch, now_ms, false);
    }

    /// As [`Producers::marked`], for a marker read back from the log when the partition was opened: if it is the first
    /// thing known of the producer, the batches it ended may be gone from the log, so its sequences are not known.
    pub(crate) fn marked_in_history(&mut self, id: i64, epoch: i16, now_ms: i64) {
        self.mark(id, epoch, now_ms, true);
    }

    fn mark(&mut self, id: i64, epoch: i16, now_ms: i64, from_history: bool) {
        match self.by_id.get_mut(&id) {
            Some(p) if p.epoch >= epoch => p.last_used_ms = now_ms,
            known => {
                // A marker above the epoch known raised it: that is a new session, which starts at 0.
                let first_of_it = known.is_none();
                self.by_id.insert(
                    id,
                    Producer {
                        epoch,
                        recent: VecDeque::new(),
                        last_used_ms: now_ms,
                        sequences_lost: from_history && first_of_it,
                    },
                );
            }
        }
    }

    /// The log's start moved up to `start`: a producer whose newest remembered batch is below it has nothing left in the
    /// log (Kafka drops such a producer's state with its batches), and starts afresh when it writes again.
    pub(crate) fn forget_below(&mut self, start: i64) {
        self.by_id
            .retain(|_, p| p.recent.back().is_none_or(|s| s.base_offset >= start));
    }

    /// How many producers are remembered.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.by_id.len()
    }
}

#[cfg(test)]
mod tests;

impl Producers {
    /// What each producer has in the log that compaction must not lose: where its last data batch ends (if it has one
    /// remembered), and its epoch (a marker's, when the last thing it wrote was one).
    pub(crate) fn lasts(&self) -> HashMap<i64, kafka_storage::compact::LastOf> {
        self.by_id
            .iter()
            .map(|(id, p)| {
                let data_last_offset = p
                    .recent
                    .back()
                    .map(|s| s.base_offset + i64::from(s.last.wrapping_sub(s.first) & i32::MAX));
                (
                    *id,
                    kafka_storage::compact::LastOf {
                        data_last_offset,
                        marker_epoch: p.epoch,
                    },
                )
            })
            .collect()
    }
}
