//! Retention: which of the oldest whole segments a partition may no longer keep.
//!
//! Enforcement deletes only: whole segments past a bound are removed and the log start advances. The active segment
//! is a candidate once it holds something (source: `UnifiedLog.deletableSegments`, `deleteSegments`): if it is the
//! one that has expired, a fresh one is started first, so a partition is never left without an active segment. The
//! decision is a pure function of sizes and ages, so an open partition and a closed one (judged from its files alone) give
//! the same answer, and an age is only worked out (it can mean reading a segment) when an age bound is set and
//! only for the segments up to the first one that is not too old.

use std::io;
use std::path::Path;

use kafka_wire::ids::{Offset, TimestampMs};

use crate::fs::{Fs, FsFile};
use crate::log::Retention;
use crate::logstart;
use crate::scan;
use crate::segment::{base_of_log, index_path, log_path};

/// How many of the oldest segments of a partition to delete. `sizes` are its segments, oldest first, the active one
/// last unless it is empty (an empty one is never deleted); `age_of(i)` is when the `i`th is judged to date from (the
/// newest timestamp of its records, or, if none has one, when it was last written: records stamped long ago, in a
/// replay or a backfill, are old at the next sweep however new the file is). First those older than the age bound go;
/// then the next oldest, each while the partition would still hold the size bound without it (so the partition is left
/// with at least the bound, as Kafka leaves it).
///
/// # Errors
/// A failure of `age_of`.
pub fn count_to_drop(
    sizes: &[u64],
    mut age_of: impl FnMut(usize) -> io::Result<i64>,
    policy: Retention,
    now: TimestampMs,
) -> io::Result<usize> {
    let mut dropped = 0;
    if let Some(max_age) = policy.max_age_ms {
        let cutoff = now.get().saturating_sub(max_age);
        while dropped < sizes.len() && age_of(dropped)? < cutoff {
            dropped += 1;
        }
    }
    if let Some(max_bytes) = policy.max_bytes {
        let mut held: u64 = sizes.iter().skip(dropped).sum();
        for size in sizes.iter().skip(dropped) {
            if held < max_bytes.saturating_add(*size) {
                break;
            }
            held -= size;
            dropped += 1;
        }
    }
    Ok(dropped)
}

/// Applies a policy to a partition that is not open, from file sizes and times alone, and returns how many
/// segments it removed. As for an open partition, the new start is recorded before any file is removed, and the
/// removals are flushed after; the log start otherwise follows from the first segment left.
///
/// # Errors
/// A failed listing, read, record or removal.
pub fn enforce_closed<F: Fs>(
    fs: &F,
    dir: &Path,
    policy: Retention,
    now: TimestampMs,
) -> io::Result<usize> {
    let mut bases: Vec<Offset> = fs
        .list_dir(dir)?
        .iter()
        .filter_map(|e| base_of_log(&e.name))
        .collect();
    bases.sort();
    let open = |base: &Offset| fs.open_existing(&log_path(dir, *base));
    let mut sizes = bases
        .iter()
        .map(|b| open(b).and_then(|f| f.size()))
        .collect::<io::Result<Vec<_>>>()?;
    // An empty active segment is never deleted.
    if sizes.last() == Some(&0) {
        sizes.pop();
    }
    let age_of = |i: usize| {
        let log = open(bases.get(i).unwrap_or(&Offset::ZERO))?;
        let size = log.size()?;
        scan::max_timestamp(&log, size)?.map_or_else(|| log.modified_ms(), |t| Ok(t.get()))
    };
    let drop = count_to_drop(&sizes, age_of, policy, now)?;
    if drop == 0 {
        return Ok(0);
    }
    let survivor = match bases.get(drop) {
        Some(next) => *next,
        // Everything goes, the active segment too: a fresh one is made at the end of the log, first, so that the log
        // is never left with no segment.
        None => begin_after(fs, dir, bases.last().copied().unwrap_or(Offset::ZERO))?,
    };
    if survivor > logstart::read(fs, dir) {
        logstart::write(fs, dir, survivor)?;
    }
    for base in bases.iter().take(drop) {
        crate::segment::remove_files(fs, dir, *base)?;
    }
    fs.sync_dir(dir)?;
    Ok(drop)
}

/// Makes an empty segment where the log that ends in the segment at `last` ends, and says where that is.
fn begin_after<F: Fs>(fs: &F, dir: &Path, last: Offset) -> io::Result<Offset> {
    let log = fs.open_existing(&log_path(dir, last))?;
    let size = log.size()?;
    let end = scan::scan(&log, size, last, u64::MAX, 0)?.next_offset;
    fs.open_or_create(&log_path(dir, end))?;
    fs.open_or_create(&index_path(dir, end))?;
    // The new names are on disk before the start is recorded past everything that is there.
    fs.sync_dir(dir)?;
    Ok(end)
}
