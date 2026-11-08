//! Retention: which of the oldest whole segments a partition may no longer keep.
//!
//! Enforcement deletes only: whole sealed segments past a bound are removed and the log start advances. The
//! active segment is never a candidate, so a partition always keeps its most recent data. The decision is a
//! pure function of sizes and ages, so an open partition and a closed one (judged from its files alone) give
//! the same answer, and an age is only worked out (it can mean reading a segment) when an age bound is set and
//! only for the segments up to the first one that is not too old.

use std::io;
use std::path::Path;

use kafka_wire::ids::{Offset, TimestampMs};

use crate::fs::{Fs, FsFile};
use crate::log::Retention;
use crate::logstart;
use crate::scan;
use crate::segment::{base_of_log, log_path};

/// How many of the oldest segments of a partition to delete. `sizes` are the sealed segments, oldest first;
/// `age_of(i)` is when the `i`th is judged to date from (the newest timestamp of its records, or, if none has
/// one, when it was last written: records stamped long ago, in a replay or a backfill, are old at the next sweep
/// however new the file is). First those older than the age bound go; then, while the partition (with its
/// `active` segment) is over the size bound, the next oldest.
///
/// # Errors
/// A failure of `age_of`.
pub(crate) fn count_to_drop(
    sizes: &[u64],
    mut age_of: impl FnMut(usize) -> io::Result<i64>,
    active: u64,
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
        let mut total = active + sizes.iter().skip(dropped).sum::<u64>();
        for size in sizes.iter().skip(dropped) {
            if total <= max_bytes {
                break;
            }
            total -= size;
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
    let Some((last, sealed_bases)) = bases.split_last() else {
        return Ok(0);
    };
    let open = |base: &Offset| fs.open_existing(&log_path(dir, *base));
    let sizes = sealed_bases
        .iter()
        .map(|b| open(b).and_then(|f| f.size()))
        .collect::<io::Result<Vec<_>>>()?;
    let active = open(last)?.size()?;
    let age_of = |i: usize| {
        let log = open(sealed_bases.get(i).unwrap_or(last))?;
        let size = log.size()?;
        scan::max_timestamp(&log, size)?.map_or_else(|| log.modified_ms(), |t| Ok(t.get()))
    };
    let drop = count_to_drop(&sizes, age_of, active, policy, now)?;
    if drop == 0 {
        return Ok(0);
    }
    let survivor = bases.get(drop).copied().unwrap_or(*last);
    if survivor > logstart::read(fs, dir) {
        logstart::write(fs, dir, survivor)?;
    }
    for base in sealed_bases.iter().take(drop) {
        crate::segment::remove_files(fs, dir, *base)?;
    }
    fs.sync_dir(dir)?;
    Ok(drop)
}
