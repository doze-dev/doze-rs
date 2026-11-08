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
use crate::scan::scan;
use crate::segment::{
    ActiveSegment, SealedSegment, base_of_index, base_of_log, log_path, remove_files,
};

/// How much of the active segment's tail has every batch's checksum verified when the previous stop vouches
/// for it. Without that vouching the whole segment is verified: what was written and not flushed is not
/// bounded by a window, and pages reach the disk in any order, so a batch whose header made it and whose body
/// did not can sit well before the tail.
const VOUCHED_CRC_WINDOW: u64 = 8 << 20;

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

/// Removes the newest segment if it is an empty leftover that begins inside the one before it. A roll that
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
        if before.end_offset().doing("checking a segment")? <= last {
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
    /// take them. What is not vouched for starts dirty, so the first [`PartitionLog::sync`] covers it.
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
        let Some((last, earlier)) = found.bases.split_last() else {
            return Self::fresh(fs, dir, config, found.tidied);
        };
        let sealed = earlier
            .iter()
            .map(|base| SealedSegment::open(&fs, &dir, *base, interval).doing("opening a segment"))
            .collect::<Result<Vec<_>, _>>()?;
        let (mut active, _) =
            ActiveSegment::open(&fs, &dir, *last, interval).doing("opening a segment")?;
        let size = active.size();
        let crc_from = match trust {
            Trust::Vouched => size.saturating_sub(VOUCHED_CRC_WINDOW),
            Trust::Unvouched => 0,
        };
        let scanned = scan(
            active.view().log_file().as_ref(),
            size,
            *last,
            interval,
            crc_from,
        )
        .doing("scanning the active segment")?;
        active
            .adopt(&scanned)
            .doing("recovering the active segment")?;

        let end = scanned.next_offset;
        let first = sealed.first().map_or(*last, SealedSegment::base);
        let start = first.max(logstart::read(&fs, &dir).min(end));
        let mut log = Self::assemble(fs, dir, config, sealed, active, (start, end));
        log.dir_dirty = found.tidied;
        // A crash between recording a new start and removing the segments below it leaves them on disk,
        // invisible. Finish the job.
        while !log.sealed.is_empty() && log.second_base() <= start {
            log.drop_oldest()?;
        }
        if trust == Trust::Unvouched {
            log.active.mark_unflushed();
            log.dir_dirty = true;
            log.append_seq = 1; // one append nobody has flushed: what was read back
        }
        Ok(log)
    }

    /// A partition with no segments yet: nothing to recover and nothing to doubt.
    fn fresh(fs: F, dir: PathBuf, config: LogConfig, tidied: bool) -> Result<Self, StorageError> {
        let (active, _) = ActiveSegment::open(&fs, &dir, Offset::ZERO, config.index_interval_bytes)
            .doing("creating the first segment")?;
        let mut log = Self::assemble(
            fs,
            dir,
            config,
            Vec::new(),
            active,
            (Offset::ZERO, Offset::ZERO),
        );
        // Its name is not flushed here: the flush that an acknowledgement waits for flushes it too.
        log.dir_dirty = true;
        log.dir_dirty |= tidied;
        Ok(log)
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
        }
    }
}
