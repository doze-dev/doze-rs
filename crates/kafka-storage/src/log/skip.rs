//! What a read hands back: the batches from an offset, without a lead of batches that hold no records.

use bytes::Bytes;
use kafka_wire::batch::Peek;
use kafka_wire::ids::Offset;

use super::LogReader;
use crate::error::StorageError;
use crate::fs::Fs;

impl<F: Fs> LogReader<F> {
    /// Whole batches starting at the one that holds `from`, up to `max_bytes` (but always the first one, if
    /// there is one, however large). An offset equal to the end is an empty answer (caught up). With a `limit`
    /// (the last stable offset, for a read-committed fetch), no batch that begins at or after it is returned.
    ///
    /// Batches with no records in them (what compaction leaves of a batch to keep its offsets) are not the first thing
    /// returned when records follow: a client that is given nothing but such a batch gets no record to count as progress,
    /// and librdkafka, given a run of those, gives up on the partition ("might be too large to fetch"). They are only
    /// returned when nothing follows them in what may be read, so that a reader at the end of the log still moves past them.
    ///
    /// # Errors
    /// [`StorageError::OutOfRange`] below the log start or above the end; a failed read.
    pub fn read(
        &self,
        from: Offset,
        max_bytes: usize,
        limit: Option<Offset>,
    ) -> Result<Bytes, StorageError> {
        let mut at = from;
        loop {
            let found = self.read_once(at, max_bytes, limit)?;
            let Some((after_empties, rest)) = skip_empty(&found) else {
                return Ok(found);
            };
            if !rest.is_empty() {
                return Ok(rest);
            }
            // Only empty batches were read. Past them there is more to read unless the log (or the limit) ends there.
            let bound = limit.map_or(self.end_offset(), |l| l.min(self.end_offset()));
            match Offset::new(after_empties) {
                Some(next) if next < bound => at = next,
                _ => return Ok(found),
            }
        }
    }
}

/// If `bytes` begin with batches that hold no records: the offset after them and what follows them. `None` if they do not.
fn skip_empty(bytes: &Bytes) -> Option<(i64, Bytes)> {
    let (mut pos, mut next) = (0usize, None);
    while let Some(head) = bytes.get(pos..) {
        let Ok(peek) = Peek::read(head) else { break };
        if peek.record_count != 0 {
            break;
        }
        next = peek.next_offset();
        pos += usize::try_from(peek.total_len).ok()?;
    }
    next.map(|n| (n, bytes.slice(pos.min(bytes.len())..)))
}

#[cfg(test)]
mod tests {
    use kafka_wire::batch::build::{BatchBuilder, Rec};
    use kafka_wire::batch::{Batch, Rules};

    use super::*;

    fn one_record() -> Bytes {
        let rec = Rec {
            key: Some(b"k".to_vec()),
            value: Some(b"v".to_vec()),
            ..Rec::default()
        };
        BatchBuilder::from_records(5, &[rec]).build()
    }

    /// A batch cleaning has emptied: its offsets are kept, its records are not.
    fn emptied() -> Bytes {
        let rules = Rules {
            max_inflated: 1 << 20,
            horizon: 0,
            marker_of_empty_transaction: false,
        };
        let batch = Batch::parse_stored(one_record()).unwrap();
        batch.filter(rules, |_| false).unwrap().batch.into_bytes()
    }

    #[test]
    fn a_run_of_batches_with_no_records_is_skipped_whole_and_what_follows_is_left() {
        let full = one_record();
        let mut all = Vec::new();
        all.extend_from_slice(&emptied());
        all.extend_from_slice(&emptied());
        all.extend_from_slice(&full);
        let (_, rest) = skip_empty(&Bytes::from(all)).expect("a lead of empty batches");
        assert_eq!(rest.len(), full.len(), "both empty batches are skipped");
        assert!(skip_empty(&full).is_none(), "no lead, nothing to skip");
    }
}
