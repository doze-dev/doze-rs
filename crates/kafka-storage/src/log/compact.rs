//! Cleaning a partition: planning a round from the segments, and putting its results in place. The round itself runs
//! elsewhere on the segments' readers (see [`crate::compact`]); only this, on the writer, changes the files.
//!
//! A cleaned segment keeps its name (its first offset), so putting it in place replaces the file. The index is not
//! trusted across that: it is removed first (a segment with no index has one made when it is opened), the log is renamed
//! over, then the index made beforehand is renamed in. A stop between any two of those leaves a segment whole and with an
//! index that is right or absent.

use std::collections::HashMap;

use kafka_wire::ids::Offset;

use super::PartitionLog;
use crate::compact::plan::{self, Facts, Params};
use crate::compact::{AbortedTxn, CleanJob, Cleaned, Input, LastOf, Replacement, temporary};
use crate::compact::{read_checkpoint, write_checkpoint};
use crate::error::{Doing, StorageError};
use crate::fs::Fs;
use crate::segment::{SealedSegment, index_path, log_path, remove_files};

impl<F: Fs> PartitionLog<F> {
    /// A round of cleaning, if the log is due one. `last_stable` is the first offset still open to a transaction;
    /// `aborted` the aborted transactions of the log and `producers` what each active producer has in it.
    ///
    /// # Errors
    /// A failed read of a segment's timestamps.
    pub fn clean_job(
        &self,
        params: &Params,
        last_stable: Offset,
        aborted: Vec<AbortedTxn>,
        producers: HashMap<i64, LastOf>,
    ) -> Result<Option<CleanJob<F>>, StorageError> {
        let ages = params.min_lag_ms > 0 || params.max_lag_ms != i64::MAX;
        let mut facts = Vec::with_capacity(self.sealed.len());
        let forced = params.max_lag_ms != i64::MAX;
        for s in &self.sealed {
            let oldest_ms = if forced {
                s.first_batch_timestamp()
                    .doing("reading a segment's first timestamp")?
            } else {
                0
            };
            let newest_ms = if ages {
                s.max_timestamp()
                    .doing("reading a segment's timestamps")?
                    .map_or(s.modified_ms(), |t| t.get())
            } else {
                0
            };
            facts.push(Facts {
                next: self
                    .sealed
                    .get(facts.len() + 1)
                    .map_or(self.active.base(), SealedSegment::base)
                    .get(),
                base: s.base().get(),
                size: s.size(),
                newest_ms,
                oldest_ms,
            });
        }
        // A checkpoint past the end of the log (the log was cut back, or the file is not this log's) is no place to go on from:
        // cleaning begins again from the start (source: `LogCleanerManager.cleanableOffsets`).
        let recorded = read_checkpoint(&self.fs, &self.dir);
        let checkpoint = if recorded > self.end {
            self.start
        } else {
            recorded.max(self.start)
        };
        let Some(plan) = plan::judge(
            &facts,
            checkpoint.get(),
            last_stable.get(),
            self.active.base().get(),
            params,
        ) else {
            return Ok(None);
        };
        let mut segments = Vec::with_capacity(self.sealed.len());
        for (i, s) in self.sealed.iter().enumerate() {
            let next = self
                .sealed
                .get(i + 1)
                .map_or(self.active.base(), SealedSegment::base);
            segments.push(Input {
                base: s.base(),
                next,
                view: s.view(),
            });
        }
        Ok(Some(CleanJob {
            fs: self.fs.clone(),
            dir: self.dir.clone(),
            segments,
            plan,
            params: *params,
            index_interval: self.config.index_interval_bytes,
            aborted,
            producers,
        }))
    }

    /// Puts a round's new segments in place, one at a time, and records how far cleaning got. A segment that is no
    /// longer as the round found it (retention removed it) is left. Returns how many were replaced.
    ///
    /// # Errors
    /// A step that failed. What is done stays done (every segment is whole at every step); the files of those not put
    /// in place are taken back.
    pub fn install_cleaned(&mut self, cleaned: &Cleaned) -> Result<usize, StorageError> {
        let mut replaced = 0;
        let mut result = Ok(());
        for r in &cleaned.replacements {
            if result.is_err() {
                self.discard(r);
                continue;
            }
            match self.install_one(r) {
                Ok(true) => replaced += 1,
                Ok(false) => {}
                Err(e) => result = Err(e),
            }
        }
        result?;
        if let (true, Some(to)) = (
            cleaned.clean_to > read_checkpoint(&self.fs, &self.dir).get(),
            Offset::new(cleaned.clean_to),
        ) {
            write_checkpoint(&self.fs, &self.dir, to).doing("recording how far cleaning got")?;
        }
        Ok(replaced)
    }

    /// Takes back the files of a round that will not be put in place.
    pub fn discard_cleaned(&self, cleaned: &Cleaned) {
        for r in &cleaned.replacements {
            self.discard(r);
        }
    }

    fn discard(&self, r: &Replacement) {
        let (log, index) = temporary(&self.dir, r.base);
        let _ = self.fs.remove_file(&log);
        let _ = self.fs.remove_file(&index);
    }

    fn install_one(&mut self, r: &Replacement) -> Result<bool, StorageError> {
        let Some(at) = self
            .sealed
            .iter()
            .position(|s| s.base() == r.base && s.size() == r.from_size)
        else {
            self.discard(r);
            return Ok(false);
        };
        if r.vanish {
            return self.remove_emptied(at);
        }
        let (log_tmp, index_tmp) = temporary(&self.dir, r.base);
        let dir = self.dir.clone();
        let swapped = (|| -> std::io::Result<()> {
            self.fs.remove_file(&index_path(&dir, r.base))?;
            self.fs.sync_dir(&dir)?;
            self.fs.rename(&log_tmp, &log_path(&dir, r.base))?;
            self.fs.sync_dir(&dir)?;
            self.fs.rename(&index_tmp, &index_path(&dir, r.base))?;
            self.fs.sync_dir(&dir)
        })();
        // Whatever happened, the files are whole: open the segment from them as they are.
        let interval = self.config.index_interval_bytes;
        match SealedSegment::open(&self.fs, &self.dir, r.base, interval) {
            Ok(fresh) => {
                if let Some(slot) = self.sealed.get_mut(at) {
                    *slot = fresh;
                }
                self.publish_segments();
            }
            // The segment the partition holds is still whole (it reads the file it was opened on, which is as it was), so
            // the partition stays online: a failure to open, for want of a descriptor say, may pass. The next open of the
            // log reads the new file.
            Err(e) => {
                self.dir_dirty = true;
                return Err(StorageError::Io {
                    doing: "opening a cleaned segment",
                    source: e,
                });
            }
        }
        swapped.doing("putting a cleaned segment in place")?;
        Ok(true)
    }

    /// Removes a sealed segment that cleaning left with nothing the log needs. The offsets it covered are a gap, as
    /// they are inside a cleaned segment: a read there goes on to the next batch. The first segment is never taken (it
    /// is where the log starts, and may have become the first since the round was planned). A file that cannot be
    /// removed stays on disk, whole and harmless: the next open finds an empty segment and the next round removes it.
    fn remove_emptied(&mut self, at: usize) -> Result<bool, StorageError> {
        if at == 0 {
            return Ok(false);
        }
        let gone = self.sealed.remove(at);
        let base = gone.base();
        drop(gone);
        self.publish_segments();
        self.dir_dirty = true;
        remove_files(&self.fs, &self.dir, base).doing("removing an emptied segment")?;
        Ok(true)
    }
}
