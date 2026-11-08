//! Finding a record by when it was written.
//!
//! There is no time index: a lookup walks the batches from the log start, looking at each batch's newest timestamp
//! and reading the records of only the one that answers. That is a read of the log up to the answer, which a
//! development broker's logs can afford and a production one's could not.

use kafka_wire::batch::{Batch, Batches};
use kafka_wire::ids::{Offset, TimestampMs};

use super::LogReader;
use crate::error::StorageError;
use crate::fs::Fs;

/// How much of the log one read of a lookup takes.
const CHUNK: usize = 1 << 20;

/// An offset and the time of the record at it.
pub type OffsetAt = (Offset, TimestampMs);

impl<F: Fs> LogReader<F> {
    /// The first record from the log start whose timestamp is at least `target`: its offset and timestamp. `None` if
    /// there is none (every record is older, or there are no records).
    ///
    /// # Errors
    /// A failed read, or a batch that is not one.
    pub fn offset_for_timestamp(
        &self,
        target: TimestampMs,
    ) -> Result<Option<OffsetAt>, StorageError> {
        let start = self.start_offset();
        let mut found = None;
        self.walk(|batch| {
            if batch.max_timestamp() < target {
                return false;
            }
            found = within(batch, start, |t| t >= target);
            found.is_some()
        })?;
        Ok(found)
    }

    /// The record with the largest timestamp (the first of several): its offset and timestamp. `None` if the log
    /// holds no records.
    ///
    /// # Errors
    /// A failed read, or a batch that is not one.
    pub fn offset_of_max_timestamp(&self) -> Result<Option<OffsetAt>, StorageError> {
        let start = self.start_offset();
        let mut best: Option<(TimestampMs, Batch)> = None;
        self.walk(|batch| {
            if best
                .as_ref()
                .is_none_or(|(t, _)| batch.max_timestamp() > *t)
            {
                best = Some((batch.max_timestamp(), batch.clone()));
            }
            false
        })?;
        Ok(best.and_then(|(newest, batch)| within(&batch, start, |t| t == newest)))
    }

    /// Calls `visit` with each batch from the log start, in order, until it says to stop.
    fn walk(&self, mut visit: impl FnMut(&Batch) -> bool) -> Result<(), StorageError> {
        let mut at = self.start_offset();
        loop {
            let chunk = self.read(at, CHUNK, None)?;
            if chunk.is_empty() {
                return Ok(());
            }
            for batch in Batches::new(chunk) {
                let batch = batch.map_err(damage)?;
                if visit(&batch) {
                    return Ok(());
                }
                let Some(next) = next_offset(&batch) else {
                    return Err(damage("a stored batch with no offset"));
                };
                at = next;
            }
        }
    }
}

/// The first record of `batch` at or after `start` whose timestamp passes `wanted`. A batch whose records cannot be
/// walked (a compressed one) answers for its first visible record with the batch's newest timestamp, the finest
/// grain its header has.
fn within(batch: &Batch, start: Offset, wanted: impl Fn(TimestampMs) -> bool) -> Option<OffsetAt> {
    let base = batch.base_offset()?;
    let by_header = || {
        let first = base.get().max(start.get());
        Some((Offset::new(first)?, batch.max_timestamp()))
    };
    let Some(records) = batch.record_times() else {
        return by_header();
    };
    for record in records {
        let Ok(record) = record else {
            return by_header();
        };
        let offset = base.get().checked_add(i64::from(record.offset_delta))?;
        if offset >= start.get() && wanted(record.timestamp) {
            return Some((Offset::new(offset)?, record.timestamp));
        }
    }
    None
}

fn next_offset(batch: &Batch) -> Option<Offset> {
    batch.base_offset()?.checked_add(batch.record_count())
}

fn damage(why: impl std::fmt::Display) -> StorageError {
    StorageError::Io {
        doing: "reading timestamps",
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, why.to_string()),
    }
}
