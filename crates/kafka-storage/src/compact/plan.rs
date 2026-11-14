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
    /// The timestamp of its first batch (0 if it has none): what `max.compaction.lag.ms` counts a segment's age from.
    pub oldest_ms: i64,
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

/// What stops cleaning short of the log's end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    /// The active segment, which is never cleaned.
    Active,
    /// The first offset still open to a transaction.
    Transaction,
    /// The first dirty segment that is newer than `min.compaction.lag.ms`.
    TooNew,
}

/// What the planner reads of a log, whether or not it is due: the numbers the rules are judged by. Read-only: the console
/// shows these beside the verdict, and [`judge`] is made from them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// The first offset that may not be cleaned, and what sets it.
    pub upto: i64,
    pub limit: Limit,
    /// The first offset not yet cleaned (the checkpoint, no lower than the first segment).
    pub first_dirty: i64,
    /// The base of the segment that holds `first_dirty`: it and what follows are dirty.
    pub dirty_start: i64,
    /// Bytes in the cleanable dirty part, and below it.
    pub dirty_bytes: u64,
    pub clean_bytes: u64,
    /// The first dirty segment's first record is older than `max.compaction.lag.ms`: due whatever the share.
    pub waited_too_long: bool,
}

impl Reading {
    /// The dirty share of the cleanable log, as the planner compares it with `min.cleanable.dirty.ratio`; `None` with
    /// nothing dirty.
    #[must_use]
    pub fn ratio(&self) -> Option<f64> {
        (self.dirty_bytes > 0)
            .then(|| self.dirty_bytes as f64 / (self.dirty_bytes + self.clean_bytes) as f64)
    }
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
    let r = reading(segments, checkpoint, unstable, active_base, p);
    if r.dirty_bytes == 0 {
        return None;
    }
    let ratio = r.dirty_bytes as f64 / (r.dirty_bytes + r.clean_bytes) as f64;
    (ratio > p.min_dirty_ratio || r.waited_too_long).then_some(Plan {
        upto: r.upto,
        first_dirty: r.first_dirty,
        dirty_bytes: r.dirty_bytes,
        clean_bytes: r.clean_bytes,
    })
}

/// What [`judge`] reads of the log, said whether or not the log is due.
#[must_use]
pub fn reading(
    segments: &[Facts],
    checkpoint: i64,
    unstable: i64,
    active_base: i64,
    p: &Params,
) -> Reading {
    let first_dirty = segments
        .first()
        .map_or(checkpoint, |s| checkpoint.max(s.base));
    // The segment that holds the first dirty offset is dirty whole; those before it are clean.
    let dirty_start = segments
        .iter()
        .rev()
        .find(|s| s.base <= first_dirty && first_dirty < s.next)
        .map_or(first_dirty, |s| s.base);
    // The first dirty segment too new to clean limits how far cleaning goes (`LogCleanerManager.cleanableOffsets`): only when
    // there is a minimum lag, and only among the dirty ones: a clean segment's age is no reason to wait.
    let too_new = segments
        .iter()
        .filter(|s| p.min_lag_ms > 0 && s.base >= dirty_start)
        .find(|s| s.newest_ms > p.now_ms.saturating_sub(p.min_lag_ms))
        .map_or(i64::MAX, |s| s.base);
    let upto = unstable.min(active_base).min(too_new);
    let limit = if too_new < unstable.min(active_base) {
        Limit::TooNew
    } else if unstable < active_base {
        Limit::Transaction
    } else {
        Limit::Active
    };
    let (mut dirty, mut clean) = (0u64, 0u64);
    for s in segments {
        if s.base < dirty_start {
            clean += s.size;
        } else if s.base < upto {
            dirty += s.size;
        }
    }
    // Forced once the earliest first-batch timestamp of the dirty segments is older than the maximum lag
    // (`LogCleanerManager.maxCompactionDelay`): a segment is as old as its first record, not its last.
    let waited_too_long = p.max_lag_ms != i64::MAX
        && segments
            .iter()
            .filter(|s| s.base >= dirty_start && s.oldest_ms > 0)
            .map(|s| s.oldest_ms)
            .min()
            .is_some_and(|earliest| earliest < p.now_ms.saturating_sub(p.max_lag_ms));
    Reading {
        upto,
        limit,
        first_dirty,
        dirty_start,
        dirty_bytes: dirty,
        clean_bytes: clean,
        waited_too_long,
    }
}

#[cfg(test)]
mod tests;
