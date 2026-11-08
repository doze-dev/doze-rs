//! Taking batches: stamping offsets, writing them, rolling to a new segment.

use kafka_wire::batch::{Batch, TimestampType};
use kafka_wire::ids::TimestampMs;

use super::{Appended, PartitionLog};
use crate::error::{Doing, StorageError};
use crate::fs::Fs;
use crate::segment::ActiveSegment;

fn exhausted() -> StorageError {
    StorageError::Io {
        doing: "assigning offsets",
        source: std::io::Error::other("the partition has no offsets left"),
    }
}

impl<F: Fs> PartitionLog<F> {
    /// Appends the batches of a Produce payload, giving each the next offsets as it is written. The batches are
    /// written from the buffers the caller holds: none is copied or changed. With `clock`
    /// [`TimestampType::LogAppend`] the batches are marked as carrying `now`, the broker's time. The payload
    /// goes in whole or not at all: if the active segment cannot take it, the roll comes first and all of it
    /// goes in the new segment; a write that fails part-way takes back the batches before it, and no offset is
    /// used. Nothing is flushed: the [`Ticket`](super::Ticket) says when it is.
    ///
    /// # Errors
    /// An offline partition, a failed write (the partition stays online and the call may be retried), or an
    /// exhausted offset space.
    pub fn append(
        &mut self,
        batches: &[Batch],
        now: TimestampMs,
        clock: TimestampType,
    ) -> Result<Appended, StorageError> {
        self.check_online()?;
        let size: u64 = batches.iter().map(|b| b.len() as u64).sum();
        let (active, limit) = (self.active.size(), self.config.segment_bytes);
        if active > 0 && active + size > limit {
            self.roll()?;
        }
        let (first, mark) = (self.end, self.active.mark());
        let broker_time = (clock == TimestampType::LogAppend).then_some(now);
        let mut next = first;
        for batch in batches {
            let stamped = batch.stamped(next, broker_time);
            if let Err(e) = self.active.append(&stamped, now.get()) {
                self.active.rewind(mark);
                return Err(e).doing("writing a batch");
            }
            next = next.checked_add(batch.record_count()).ok_or_else(|| {
                self.active.rewind(mark);
                exhausted()
            })?;
        }
        self.active.publish();
        self.end = next;
        self.append_seq += 1;
        self.shared.end.send_replace(next);
        Ok(Appended {
            first_offset: first,
            next_offset: next,
            ticket: self.ticket_for(self.append_seq),
        })
    }

    /// Seals the active segment and starts the one that begins at the log-end offset. The sealed segment is
    /// flushed first, its contents and its name: from here nothing looks at it again, not the timer, not an
    /// acknowledgement, and not recovery, so this is the flush that every record in it rests on. Flushing the
    /// name too means the names that exist after a power cut are always a run of segments up to the newest: a
    /// newer segment never outlives an older one, so the log has no hole in the middle.
    fn roll(&mut self) -> Result<(), StorageError> {
        self.flush_active()?;
        self.flush_directory()?;
        self.start_segment()
    }

    /// Makes a new active segment at the log-end offset, sealing the old one (already flushed by the caller).
    /// The new name is not flushed here: it makes the directory dirty, and the next sync flushes it with
    /// whatever is acknowledged in the segment.
    pub(super) fn start_segment(&mut self) -> Result<(), StorageError> {
        let base = self.end;
        let opened =
            ActiveSegment::open(&self.fs, &self.dir, base, self.config.index_interval_bytes);
        let (fresh, created) = match opened {
            Ok(opened) => opened,
            Err(e) => {
                // A failure after the log was created has removed it again, but the creation and the removal
                // are only in memory: the next sync must flush the directory, so that a power cut cannot bring
                // back an empty segment that the old one's later appends then overlap.
                self.dir_dirty = true;
                return Err(e).doing("starting a segment");
            }
        };
        let old = std::mem::replace(&mut self.active, fresh);
        self.sealed.push(old.into_sealed());
        self.dir_dirty |= created;
        self.publish_segments();
        Ok(())
    }
}
