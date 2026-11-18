//! A share-partition to and from what is stored (Kafka's share coordinator state): the start, and each batch in flight
//! with its state and delivery count. What is acquired is stored as it was before (Kafka writes no acquisition).

use kafka_meta::{ShareBatch, ShareRecordState, ShareState};

use super::{GapWindow, InFlight, Rec, RecordState, SharePartition, Unreadable};

impl SharePartition {
    /// The share-partition as it was stored (Kafka's `maybeInitialize`): what was acquired then is available now, the
    /// gaps between stored batches are fetched again.
    ///
    /// # Errors
    /// A stored batch before the start, or one that ends before it begins.
    pub(crate) fn restore(state: &ShareState) -> Result<Self, Unreadable> {
        let mut p = Self::new(state.start_offset, state.state_epoch);
        let mut gap_start = None;
        let mut previous_last = p.start - 1;
        for b in &state.batches {
            if b.first < p.start {
                return Err(Unreadable("a stored batch begins before the start offset"));
            }
            if b.last < b.first {
                return Err(Unreadable("a stored batch ends before it begins"));
            }
            if gap_start.is_none() && b.first > previous_last + 1 {
                gap_start = Some(previous_last + 1);
            }
            previous_last = b.last;
            let state = match b.state {
                ShareRecordState::Available => RecordState::Available,
                ShareRecordState::Acknowledged => RecordState::Acknowledged,
                ShareRecordState::Archived => RecordState::Archived,
            };
            p.cached.insert(
                b.first,
                InFlight::whole(b.last, Rec::new(state, b.delivery_count)),
            );
        }
        if let Some((_, last)) = p.cached.last_key_value() {
            p.find_next = true;
            p.end = last.last;
            p.gap = gap_start.map(|gap_start| GapWindow {
                end: p.end,
                gap_start,
            });
            p.advance();
        }
        Ok(p)
    }

    /// What is stored: the start, and from it every record that is not simply waiting for its first delivery. A record held
    /// now is stored as it was before it was acquired (available, one delivery fewer): Kafka does not write an acquisition,
    /// so a lock never outlives the broker and a delivery that was never acknowledged is not counted after a restart.
    #[must_use]
    pub(crate) fn snapshot(&self) -> ShareState {
        let mut batches: Vec<ShareBatch> = Vec::new();
        for (&first, b) in &self.cached {
            for (offset, r) in b.records(first) {
                if offset < self.start {
                    continue;
                }
                let stored = match r.state {
                    RecordState::Available if r.count > 0 => {
                        Some((ShareRecordState::Available, r.count))
                    }
                    RecordState::Acquired if r.count > 1 => {
                        Some((ShareRecordState::Available, r.count - 1))
                    }
                    RecordState::Acknowledged => Some((ShareRecordState::Acknowledged, r.count)),
                    RecordState::Archived => Some((ShareRecordState::Archived, r.count)),
                    RecordState::Available | RecordState::Acquired => None,
                };
                let Some((state, delivery_count)) = stored else {
                    continue;
                };
                match batches.last_mut() {
                    Some(last)
                        if last.last + 1 == offset
                            && last.state == state
                            && last.delivery_count == delivery_count =>
                    {
                        last.last = offset;
                    }
                    _ => batches.push(ShareBatch {
                        first: offset,
                        last: offset,
                        state,
                        delivery_count,
                    }),
                }
            }
        }
        ShareState {
            state_epoch: self.state_epoch,
            start_offset: self.start,
            batches,
        }
    }
}
