//! Whether a log is due to be cleaned, and how far (source: `LogCleanerManager.cleanableOffsets`, `LogToClean`): the part
//! of the log below the first dirty offset has been cleaned already; the dirty part is cleaned once it is a large enough
//! share of the log, or has waited longer than the maximum compaction lag; and what is too new, still open to a
//! transaction, or the active segment, is left alone.

/// What the topic asks of compaction, and the time.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// Clean once the dirty bytes are more than this share of the log (`min.cleanable.dirty.ratio`).
    pub min_dirty_ratio: f64,
    /// A record is not cleaned until it is this old (`min.compaction.lag.ms`).
    pub min_lag_ms: i64,
    /// A record is cleaned once it is this old whatever the ratio (`max.compaction.lag.ms`; `i64::MAX`: never forced).
    pub max_lag_ms: i64,
    /// How long a deletion or a marker is kept once it is cleaned (`delete.retention.ms`).
    pub delete_retention_ms: i64,
    /// The time now.
    pub now_ms: i64,
    /// The most bytes of keys the offset map may hold; more than that is cleaned in a later round.
    pub map_bytes: usize,
}

/// A sealed segment as the plan sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    /// Its first offset.
    pub base: i64,
    /// The offset after its last batch.
    pub next: i64,
    /// Its size in bytes.
    pub size: u64,
    /// The newest timestamp in it (its last write, for one with none).
    pub newest_ms: i64,
}

/// What a plan found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    /// The first offset that may not be cleaned: the active segment, the first still-open transaction, or the first
    /// segment too new.
    pub upto: i64,
    /// The first offset not yet cleaned.
    pub first_dirty: i64,
    /// Bytes in the cleanable dirty part.
    pub dirty_bytes: u64,
    /// Bytes below it.
    pub clean_bytes: u64,
}

/// The cleanable range of the log, and whether it is worth cleaning now. `None`: nothing to do.
///
/// `segments` are the sealed segments, oldest first; `checkpoint` is where cleaning last got to; `unstable` is the first
/// offset still open to a transaction (the log end if none); `active_base` is the active segment's base.
#[must_use]
pub fn judge(
    segments: &[Facts],
    checkpoint: i64,
    unstable: i64,
    active_base: i64,
    p: &Params,
) -> Option<Plan> {
    let first_dirty = segments
        .first()
        .map_or(checkpoint, |s| checkpoint.max(s.base));
    // The segment that holds the first dirty offset is dirty whole; those before it are clean.
    let dirty_start = segments
        .iter()
        .rev()
        .find(|s| s.base <= first_dirty && first_dirty < s.next)
        .map_or(first_dirty, |s| s.base);
    // The first segment too new to clean limits how far cleaning goes.
    let too_new = segments
        .iter()
        .find(|s| s.newest_ms > p.now_ms.saturating_sub(p.min_lag_ms))
        .map_or(i64::MAX, |s| s.base);
    let upto = unstable.min(active_base).min(too_new);
    let (mut dirty, mut clean) = (0u64, 0u64);
    for s in segments {
        if s.base < dirty_start {
            clean += s.size;
        } else if s.base < upto {
            dirty += s.size;
        }
    }
    if dirty == 0 {
        return None;
    }
    let ratio = dirty as f64 / (dirty + clean) as f64;
    let waited_too_long = segments
        .iter()
        .find(|s| s.base >= dirty_start && s.base < upto)
        .is_some_and(|s| {
            p.max_lag_ms != i64::MAX && p.now_ms.saturating_sub(s.newest_ms) >= p.max_lag_ms
        });
    (ratio > p.min_dirty_ratio || waited_too_long).then_some(Plan {
        upto,
        first_dirty,
        dirty_bytes: dirty,
        clean_bytes: clean,
    })
}

#[cfg(test)]
mod tests;
