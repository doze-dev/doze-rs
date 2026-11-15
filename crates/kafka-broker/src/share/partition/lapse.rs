//! What time and the log do to a share-partition: a lock that runs out (Kafka's `releaseAcquisitionLockOnTimeout`), and
//! a log start that moves past the start offset (Kafka's `updateCacheAndOffsets`).

use super::ack::Acked;
use super::{Count, Limits, RecordState, SharePartition, Track, offset_of};

impl SharePartition {
    /// Every record whose lock has run out by `now` is available again, or archived if it has been delivered as often as
    /// allowed or is behind the start. Kafka keeps the change even if writing it fails.
    pub(crate) fn expire(&mut self, now: i64, limits: &Limits) -> Acked {
        let start = self.start;
        let mut acked = Acked::default();
        let due = |r: &super::Rec| r.state == RecordState::Acquired && r.until.is_some_and(|u| u <= now);
        for (&key, b) in &mut self.cached {
            if let Track::Whole(r) = &b.track
                && due(r)
                && key < start
                && b.last >= start
            {
                b.explode(key);
            }
            match &mut b.track {
                Track::Whole(r) => {
                    if !due(r) {
                        continue;
                    }
                    let to = if b.last < start {
                        RecordState::Archived
                    } else {
                        RecordState::Available
                    };
                    lapse(r, to, limits, &mut acked);
                }
                Track::Each(records) => {
                    for (i, r) in records.iter_mut().enumerate() {
                        if !due(r) {
                            continue;
                        }
                        let to = if key + offset_of(i) < start {
                            RecordState::Archived
                        } else {
                            RecordState::Available
                        };
                        lapse(r, to, limits, &mut acked);
                    }
                }
            }
        }
        if acked.released {
            self.find_next = true;
        }
        acked
    }

    /// The log now starts at `log_start`: what is available before it is archived and the start offset moves up to it.
    /// Nothing is written for it (Kafka writes the new start with the next change).
    pub(crate) fn log_start_moved(&mut self, log_start: i64) {
        if log_start <= self.start {
            return;
        }
        if self.cached.is_empty() {
            self.start = log_start;
            self.end = log_start;
            return;
        }
        let keys: Vec<i64> = self.cached.keys().copied().collect();
        if self.archive_records((self.start, log_start), &keys, RecordState::Available) {
            self.find_next = true;
        }
        self.start = log_start;
        if self.end < self.start {
            self.end = self.start;
        }
    }
}

fn lapse(r: &mut super::Rec, to: RecordState, limits: &Limits, acked: &mut Acked) {
    if let Some(next) = r.moved(to, Count::Same, limits.max_delivery, None) {
        acked.changed = true;
        acked.released |= next.state == RecordState::Available;
        *r = next;
    }
}
