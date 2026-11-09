//! Opening a partition: finding its segments, recovering the active one, and working out what of it this
//! process may treat as already on disk.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kafka_wire::ids::Offset;

use super::shared::Shared;
use super::{LogConfig, PartitionLog, Trust};
use crate::error::{Doing, StorageError};
use crate::fs::Fs;
use crate::fs::FsFile;
use crate::logstart;
use crate::scan::{scan, scan_tail};
use crate::segment::{
    ActiveSegment, SealedSegment, base_of_index, base_of_log, log_path, remove_files,
};

/// What a directory held.
struct Found {
    bases: Vec<Offset>,
    /// Names were removed, so the directory's entries changed.
    tidied: bool,
}

/// Removes what an interrupted operation left (temporary files, an index with no log) and lists the segments.
fn tidy<F: Fs>(fs: &F, dir: &Path) -> Result<Found, StorageError> {
    let entries = fs.list_dir(dir).doing("listing the partition directory")?;
    let mut bases: Vec<Offset> = entries
        .iter()
        .filter_map(|e| base_of_log(&e.name))
        .collect();
    bases.sort();
    let mut tidied = false;
    for entry in &entries {
        let stray_index =
            base_of_index(&entry.name).is_some_and(|b| bases.binary_search(&b).is_err());
        if entry.name.ends_with(".tmp") || stray_index {
            tidied |= fs
                .remove_file(&dir.join(&entry.name))
                .doing("removing a leftover file")?;
        }
    }
    Ok(Found { bases, tidied })
}

/// Removes the newest segment if it is an empty leftover of a roll that failed: one that begins inside the one before it,
/// or that follows one with a torn tail. A roll that
/// failed half way (the log created, the index could not be) cleans up after itself, but the removal is only in
/// memory until the directory is flushed, and a power cut can bring the empty file back while the old segment
/// went on taking appends. Left alone it would make the log end at the leftover's base and hide the records
/// after it. Only an empty segment can be such a leftover: appends never go to a segment that failed to start.
fn drop_stale_tail<F: Fs>(
    fs: &F,
    dir: &Path,
    found: &mut Found,
    interval: u64,
) -> Result<(), StorageError> {
    while let [.., previous, last] = found.bases.as_slice() {
        let (previous, last) = (*previous, *last);
        let empty = fs
            .open_existing(&log_path(dir, last))
            .and_then(|f| f.size())
            .doing("checking a segment")?
            == 0;
        if !empty {
            return Ok(());
        }
        let before = SealedSegment::open(fs, dir, previous, interval).doing("opening a segment")?;
        let (end, valid_to) = before.tail().doing("checking a segment")?;
        // A real successor is only made after its predecessor was flushed whole. So an empty one is a leftover if
        // the predecessor goes on past where it begins, or if the predecessor has bytes that are not a batch after
        // its last: appends went on in it after the roll failed, and a power cut tore the last of them.
        if end <= last && valid_to == before.size() {
            return Ok(());
        }
        remove_files(fs, dir, last).doing("removing a stale segment")?;
        found.bases.pop();
        found.tidied = true;
    }
    Ok(())
}

fn make_dir<F: Fs>(fs: &F, dir: &Path) -> Result<(), StorageError> {
    let made = fs
        .create_dir(dir)
        .doing("creating the partition directory")?;
    if let (true, Some(parent)) = (made, dir.parent()) {
        // The new directory's own entry, in its parent.
        fs.sync_dir(parent).doing("flushing the data directory")?;
    }
    Ok(())
}

impl<F: Fs> PartitionLog<F> {
    /// Opens (creating if need be) the partition in `dir`, recovering what a crash left. The active segment is
    /// walked batch by batch and cut at the first that is not whole: an interrupted append, a zero-filled tail
    /// after a power cut, a body that never fully reached the disk. The index is made to agree with the log,
    /// which wins. Segments below the recorded log start are removed.
    ///
    /// Nothing read back is taken to be on disk unless `trust` vouches for it: the previous process may have
    /// been killed with these bytes in the page cache, where they read back perfectly and a power cut would
    /// take them. What is not vouched for starts dirty, so the first [`PartitionLog::sync`] covers it. A
    /// vouched log was flushed whole, so only its end is walked.
    ///
    /// If the recorded log start is beyond the end of the records that survived (DeleteRecords is recorded
    /// durably, the records it covered were not), the log carries on from the recorded start: offsets below it
    /// are never handed out again.
    ///
    /// # Errors
    /// A failed file operation.
    pub fn open(
        fs: F,
        dir: PathBuf,
        config: LogConfig,
        trust: Trust,
    ) -> Result<Self, StorageError> {
        make_dir(&fs, &dir)?;
        let mut found = tidy(&fs, &dir)?;
        let interval = config.index_interval_bytes;
        drop_stale_tail(&fs, &dir, &mut found, interval)?;
        let recorded = logstart::read(&fs, &dir);
        let Some((last, earlier)) = found.bases.split_last() else {
            return Self::fresh(fs, dir, config, found.tidied, recorded);
        };
        let sealed = earlier
            .iter()
            .map(|base| SealedSegment::open(&fs, &dir, *base, interval).doing("opening a segment"))
            .collect::<Result<Vec<_>, _>>()?;
        let (mut active, _) =
            ActiveSegment::open(&fs, &dir, *last, interval).doing("opening a segment")?;
        let end = recover_active(&mut active, *last, interval, trust)?;
        let first = sealed.first().map_or(*last, SealedSegment::base);
        let mut log = Self::assemble(fs, dir, config, sealed, active, (first, end));
        log.transactional = crate::transactional::exists(&log.fs, &log.dir);
        log.dir_dirty = found.tidied;
        log.carry_on_from(recorded)?;
        log.finish_interrupted_deletes()?;
        if trust == Trust::Unvouched {
            log.active.mark_unflushed();
            log.dir_dirty = true;
            log.append_seq = 1; // one append nobody has flushed: what was read back
        }
        Ok(log)
    }

    /// A partition with no segments yet: nothing to recover and nothing to doubt. If DeleteRecords had recorded
    /// a start, the first segment begins there.
    fn fresh(
        fs: F,
        dir: PathBuf,
        config: LogConfig,
        tidied: bool,
        recorded: Offset,
    ) -> Result<Self, StorageError> {
        let (active, _) = ActiveSegment::open(&fs, &dir, recorded, config.index_interval_bytes)
            .doing("creating the first segment")?;
        let mut log = Self::assemble(fs, dir, config, Vec::new(), active, (recorded, recorded));
        log.transactional = crate::transactional::exists(&log.fs, &log.dir);
        // Its name is not flushed here: the flush that an acknowledgement waits for flushes it too.
        log.dir_dirty = true;
        log.dir_dirty |= tidied;
        Ok(log)
    }

    /// Makes the log begin no earlier than `recorded` and end no earlier: a start recorded durably can outlive
    /// the records it covered (they were never flushed), and the log must not hand out those offsets again. The
    /// segment holding what survived is flushed and sealed, its name too, and a new one begins at `recorded`.
    fn carry_on_from(&mut self, recorded: Offset) -> Result<(), StorageError> {
        if recorded > self.end {
            self.flush_active()?;
            self.flush_directory()?;
            self.end = recorded;
            self.start_segment()?;
            self.shared.end.send_replace(recorded);
        }
        if recorded > self.start {
            self.start = recorded;
            self.shared.set_start(recorded);
        }
        Ok(())
    }

    /// A crash between recording a new start and removing the segments below it leaves them on disk, invisible.
    /// Finish the job. The start is recorded again first: this process read it from the page cache, and a power
    /// cut after the removals must not bring back a start that does not cover the hole they leave.
    fn finish_interrupted_deletes(&mut self) -> Result<(), StorageError> {
        if self.sealed.is_empty() || self.second_base() > self.start {
            return Ok(());
        }
        logstart::write(&self.fs, &self.dir, self.start).doing("recording the log start")?;
        while !self.sealed.is_empty() && self.second_base() <= self.start {
            self.drop_oldest()?;
        }
        Ok(())
    }

    fn assemble(
        fs: F,
        dir: PathBuf,
        config: LogConfig,
        sealed: Vec<SealedSegment<F>>,
        active: ActiveSegment<F>,
        (start, end): (Offset, Offset),
    ) -> Self {
        let mut views: Vec<_> = sealed.iter().map(SealedSegment::view).collect();
        views.push(active.view());
        let shared = Arc::new(Shared::new(views, start, end));
        Self {
            fs,
            dir,
            config,
            sealed,
            active,
            end,
            start,
            shared,
            append_seq: 0,
            synced_seq: 0,
            dir_dirty: false,
            offline: None,
            transactional: false,
        }
    }

    /// Whether this log holds, or may hold, transactional batches or transaction markers. If not, it has no transactions
    /// and is not to be read to find out.
    #[must_use]
    pub fn holds_transactions(&self) -> bool {
        self.transactional
    }

    /// Notes, durably and before the first is written, that this log is to hold a transactional batch or a transaction
    /// marker. A note already made is not made again.
    ///
    /// # Errors
    /// A failed write of the note, which takes the log offline like any failed flush.
    pub fn note_transactions(&mut self) -> Result<(), StorageError> {
        if self.transactional {
            return Ok(());
        }
        if let Err(e) = crate::transactional::record(&self.fs, &self.dir) {
            return Err(self.fail("recording that the log holds transactions", e));
        }
        self.transactional = true;
        Ok(())
    }
}

/// Brings the active segment to a consistent state and returns the offset after its last good batch.
///
/// Not vouched for, the whole segment is walked and every batch's checksum checked. Vouched for, it was flushed
/// whole by the stop that vouches, so only the stretch from the last index entry to the end is walked (checksums
/// and all); if that stretch is not whole after all, nothing is trusted and the whole segment is walked.
fn recover_active<F: Fs>(
    active: &mut ActiveSegment<F>,
    base: Offset,
    interval: u64,
    trust: Trust,
) -> Result<Offset, StorageError> {
    let size = active.size();
    let log = active.view().log_file().clone();
    if trust == Trust::Vouched && size > 0 {
        let last = active.last_index_entry().doing("reading the index")?;
        let tail = match last {
            Some(entry) => {
                scan_tail(log.as_ref(), size, entry).doing("scanning the active segment")?
            }
            None => None,
        };
        if let Some(tail) = tail {
            active.adopt_tail(&tail);
            return Ok(tail.next_offset);
        }
    }
    let scanned =
        scan(log.as_ref(), size, base, interval, 0).doing("scanning the active segment")?;
    active
        .adopt(&scanned)
        .doing("recovering the active segment")?;
    Ok(scanned.next_offset)
}
