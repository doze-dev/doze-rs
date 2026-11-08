//! Retention: which of the oldest whole segments a partition may no longer keep.
//!
//! Enforcement deletes only: whole sealed segments past a bound are removed and the log start advances. The
//! active segment is never a candidate, so a partition always keeps its most recent data. The decision is a
//! pure function of sizes and ages, so an open partition and a closed one (judged from its files alone) give
//! the same answer.

use std::io;
use std::path::Path;

use kafka_wire::ids::{Offset, TimestampMs};

use crate::fs::{Fs, FsFile};
use crate::log::Retention;
use crate::scan;
use crate::segment::{base_of_log, log_path};

/// A sealed segment as retention sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub size: u64,
    /// When it is judged to date from: the newest timestamp of its records, or, if none has one, when it was
    /// last written. Records stamped long ago (a replay, a backfill) are old at the next sweep however new the
    /// file is.
    pub age_ms: i64,
}

/// How many of the oldest segments of `sealed` (oldest first) to delete: first those older than the age bound,
/// then, while the partition (with its `active` segment) is over the size bound, the next oldest.
pub(crate) fn count_to_drop(
    sealed: &[Candidate],
    active: u64,
    policy: Retention,
    now: TimestampMs,
) -> usize {
    let mut dropped = 0;
    if let Some(max_age) = policy.max_age_ms {
        let cutoff = now.get().saturating_sub(max_age);
        dropped = sealed.iter().take_while(|c| c.age_ms < cutoff).count();
    }
    if let Some(max_bytes) = policy.max_bytes {
        let mut total = active + sealed.iter().skip(dropped).map(|c| c.size).sum::<u64>();
        for candidate in sealed.iter().skip(dropped) {
            if total <= max_bytes {
                break;
            }
            total -= candidate.size;
            dropped += 1;
        }
    }
    dropped
}

/// Applies a policy to a partition that is not open, from file sizes and times alone, and returns how many
/// segments it removed. The log start follows from the first surviving segment the next time it is opened.
///
/// # Errors
/// A failed listing, read or removal.
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
    let Some((_, sealed_bases)) = bases.split_last() else {
        return Ok(0);
    };
    let mut sealed = Vec::new();
    for base in sealed_bases {
        let log = fs.open_existing(&log_path(dir, *base))?;
        let size = log.size()?;
        let age_ms = scan::max_timestamp(&log, size)?.map_or(log.modified_ms()?, TimestampMs::get);
        sealed.push(Candidate { size, age_ms });
    }
    let active = match bases.last() {
        Some(base) => fs.open_existing(&log_path(dir, *base))?.size()?,
        None => 0,
    };
    let drop = count_to_drop(&sealed, active, policy, now);
    for base in sealed_bases.iter().take(drop) {
        crate::segment::remove_files(fs, dir, *base)?;
    }
    if drop > 0 {
        fs.sync_dir(dir)?;
    }
    Ok(drop)
}
