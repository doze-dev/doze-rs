//! Moving the start of the log: DeleteRecords, and retention.

use kafka_wire::ids::{Offset, TimestampMs};

use super::PartitionLog;
use crate::error::{Doing, StorageError};
use crate::fs::Fs;
use crate::logstart;
use crate::retention::{self, Candidate};
use crate::segment::{SealedSegment, remove_files};

/// Bounds on a partition's disk use. Each is optional; the active segment is never deleted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Retention {
    /// Delete the oldest sealed segments while the partition holds more than this many bytes.
    pub max_bytes: Option<u64>,
    /// Delete sealed segments whose newest record (or, with none, last write) is older than this.
    pub max_age_ms: Option<i64>,
}

impl Retention {
    /// Whether any bound is set.
    #[must_use]
    pub fn is_set(&self) -> bool {
        self.max_bytes.is_some() || self.max_age_ms.is_some()
    }
}

impl<F: Fs> PartitionLog<F> {
    /// Makes `to` the log start (`None` is the log end, which empties the partition): records below it are no
    /// longer served, and whole segments below it are removed. An offset at or below the start changes nothing.
    ///
    /// The new start is recorded durably before any file is removed, so a crash part-way leaves segments that
    /// are merely invisible, and the next open finishes the job. A segment that cannot be removed now does not
    /// fail the call. Returns the start now in force.
    ///
    /// # Errors
    /// [`StorageError::OutOfRange`] above the log end; an offline partition (refused before anything is
    /// recorded); a failed flush or write of the record.
    pub fn delete_before(&mut self, to: Option<Offset>) -> Result<Offset, StorageError> {
        let target = to.unwrap_or(self.end);
        if target > self.end {
            return Err(StorageError::OutOfRange {
                requested: target.get(),
                start: self.start.get(),
                end: self.end.get(),
            });
        }
        if target <= self.start {
            return Ok(self.start);
        }
        self.check_online()?;
        // Emptying a partition with records in its active segment: a fresh segment is started at the end so
        // the old one can go too, rather than holding what was deleted until the segment happens to roll. The
        // old one is flushed first, while failing still leaves nothing changed.
        let empties = target == self.end && self.active.size() > 0;
        if empties {
            self.flush_active()?;
        }
        logstart::write(&self.fs, &self.dir, target).doing("recording the log start")?;
        self.start = target;
        self.shared.set_start(target);
        if empties {
            // The start has moved, durably, and that is the answer whatever happens to the files below it:
            // the old segment stays on disk, invisible, until the partition rolls or is opened.
            if self.start_segment().is_err() {
                return Ok(self.start);
            }
        }
        while self
            .sealed
            .first()
            .is_some_and(|_| self.second_base() <= target)
        {
            if self.drop_oldest().is_err() {
                break;
            }
        }
        Ok(self.start)
    }

    /// Deletes whole sealed segments past the policy's bounds, oldest first. Returns how many it removed. The
    /// log start advances to the first segment left.
    ///
    /// # Errors
    /// A failed removal (what was already removed stays removed).
    pub fn enforce_retention(
        &mut self,
        policy: Retention,
        now: TimestampMs,
    ) -> Result<usize, StorageError> {
        if !policy.is_set() {
            return Ok(0);
        }
        let candidates: Vec<Candidate> = self
            .sealed
            .iter()
            .map(|s| Candidate {
                size: s.size(),
                age_ms: s
                    .max_timestamp()
                    .ok()
                    .flatten()
                    .map_or(s.modified_ms(), TimestampMs::get),
            })
            .collect();
        let drop = retention::count_to_drop(&candidates, self.active.size(), policy, now);
        if drop == 0 {
            return Ok(0);
        }
        self.check_online()?;
        // The new start is recorded before any file is removed, as for DeleteRecords. Removals are only in
        // memory until the directory is flushed, and a power cut may keep a later one and lose an earlier
        // one, which would leave a hole in the log; with the start recorded, whatever comes back is below it
        // and the next open removes it.
        let survivor = self
            .sealed
            .get(drop)
            .map_or(self.active.base(), SealedSegment::base);
        if survivor > self.start {
            logstart::write(&self.fs, &self.dir, survivor).doing("recording the log start")?;
        }
        for _ in 0..drop {
            self.drop_oldest()?;
        }
        Ok(drop)
    }

    /// The base of the segment after the oldest.
    pub(super) fn second_base(&self) -> Offset {
        match self.sealed.get(1) {
            Some(s) => s.base(),
            None => self.active.base(),
        }
    }

    /// Removes the oldest sealed segment from the partition and its files from disk, and moves the start up to
    /// the new first segment if it was below. The segment is gone from the partition whether or not its files
    /// could be removed: a file left behind is below the log start, and the next open removes it.
    pub(super) fn drop_oldest(&mut self) -> Result<(), StorageError> {
        if self.sealed.is_empty() {
            return Ok(());
        }
        let oldest = self.sealed.remove(0);
        let base = oldest.base();
        drop(oldest);
        let first = self.sealed.first().map_or(self.active.base(), |s| s.base());
        self.start = self.start.max(first);
        self.shared.set_start(self.start);
        self.publish_segments();
        self.dir_dirty = true;
        remove_files(&self.fs, &self.dir, base).doing("removing a segment")
    }
}
