//! Moving the share-partition start (SPSO) past what is done with (Kafka's `canMoveStartOffset`, `findLastOffsetAcknowledged`
//! and `maybeUpdateCachedStateAndOffsets`).

use super::{RecordState, SharePartition, Track, index_of, offset_of};

impl SharePartition {
    /// Kafka's `canMoveStartOffset`: the record at the start is done with.
    fn can_move_start(&self) -> bool {
        let Some((&first, b)) = self.cached.range(..=self.start).next_back() else {
            return false;
        };
        let state = match &b.track {
            Track::Whole(r) => Some(r.state),
            Track::Each(v) => v.get(index_of(first, self.start)).map(|r| r.state),
        };
        state.is_some_and(RecordState::terminal)
    }

    /// Kafka's `findLastOffsetAcknowledgedAndMetadata`: the last offset of the unbroken run of records done with, from the
    /// first in flight.
    fn last_done(&self) -> Option<i64> {
        let mut last = None;
        for (&first, b) in &self.cached {
            if self.gap_active() && self.gap.is_some_and(|g| b.last >= g.gap_start) {
                return last;
            }
            match &b.track {
                Track::Whole(r) => {
                    if !r.state.terminal() {
                        return last;
                    }
                    last = Some(b.last);
                }
                Track::Each(v) => {
                    for (i, r) in v.iter().enumerate() {
                        if !r.state.terminal() {
                            return last;
                        }
                        last = Some(first + offset_of(i));
                    }
                }
            }
        }
        last
    }

    /// Moves the start past the records done with, and lets go of the batches wholly behind it (Kafka's
    /// `maybeUpdateCachedStateAndOffsets`). `true`: it moved.
    pub(crate) fn advance(&mut self) -> bool {
        if !self.can_move_start() {
            return false;
        }
        let Some(done) = self.last_done() else {
            return false;
        };
        let Some(last_cached) = self.cached.last_key_value().map(|(_, b)| b.last) else {
            return false;
        };
        if done == last_cached {
            self.start = last_cached + 1;
            self.end = last_cached + 1;
            self.cached.clear();
            return true;
        }
        let Some((&floor, floor_batch)) = self.cached.range(..=done).next_back() else {
            return false;
        };
        let first_key = self.cached.first_key_value().map_or(floor, |(k, _)| *k);
        let remove_through = if done == floor_batch.last {
            if let Some((&higher, _)) = self.cached.range(done + 1..).next()
                && higher > self.start
            {
                self.start = higher;
            }
            if self.gap_active()
                && let Some(g) = self.gap
            {
                self.start = self.start.min(g.gap_start);
            }
            Some(floor)
        } else {
            if done + 1 > self.start {
                self.start = done + 1;
            }
            if floor == first_key {
                None
            } else {
                self.cached.range(..floor).next_back().map(|(k, _)| *k)
            }
        };
        if let Some(through) = remove_through {
            let keep = self.cached.split_off(&(through + 1));
            self.cached = keep;
        }
        true
    }
}
