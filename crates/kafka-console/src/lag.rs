//! Lag arithmetic: the log end minus the committed offset, and the cases where that subtraction is not the answer.
//!
//! The broker keeps no lag. It is worked out here from two answers, OffsetFetch (what the group committed) and ListOffsets
//! (where the log ends). Three cases are not a subtraction and must never show as a number:
//!
//! - no commit: OffsetFetch answers -1 for a partition the group never committed. It is not lag `end`; a consumer would start
//!   where its own `auto.offset.reset` says, which the broker cannot know;
//! - a commit beyond the end: Kafka accepts any offset in a commit, so a topic recreated under an app that kept its old position
//!   leaves a commit past the new log. The difference is negative, and lag is never negative;
//! - a partition (or its topic) that is not there any more: nothing to subtract from.

/// What a partition's lag is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lag {
    /// The group is behind by this many records (at least 1).
    Behind(i64),
    /// Committed at the end of the log.
    CaughtUp,
    /// The commit is this many records beyond the log end.
    PastEnd(i64),
    /// Nothing was ever committed for the partition.
    NoCommit,
    /// The partition has no log end to subtract from (the topic or partition is gone).
    Gone,
}

impl Lag {
    /// The number a total adds: only a group that is behind has any.
    #[must_use]
    pub const fn behind(self) -> i64 {
        match self {
            Self::Behind(n) => n,
            _ => 0,
        }
    }
}

/// The lag of one partition. `committed` is OffsetFetch's number (negative: none); `end` is the log end from ListOffsets
/// (`None`: the partition could not be asked).
#[must_use]
pub fn lag(committed: i64, end: Option<i64>) -> Lag {
    if committed < 0 {
        return Lag::NoCommit;
    }
    let Some(end) = end else {
        return Lag::Gone;
    };
    match committed.cmp(&end) {
        std::cmp::Ordering::Less => Lag::Behind(end.saturating_sub(committed)),
        std::cmp::Ordering::Equal => Lag::CaughtUp,
        std::cmp::Ordering::Greater => Lag::PastEnd(committed.saturating_sub(end)),
    }
}

/// How full a partition's bar is, in whole per cent: the share of the log end that is behind, at least 2 for any lag (so it is
/// seen) and 100 at most. Zero for a group that is not behind.
#[must_use]
pub fn bar_percent(lag: Lag, end: Option<i64>) -> u8 {
    let (Lag::Behind(n), Some(end)) = (lag, end) else {
        return 0;
    };
    if end <= 0 {
        return 100;
    }
    let share = i128::from(n) * 100 / i128::from(end);
    u8::try_from(share.clamp(2, 100)).unwrap_or(100)
}

/// A bar for a total against the largest total: `part` out of `of`, with the same floor and ceiling.
#[must_use]
pub fn share_percent(part: i64, of: i64) -> u8 {
    if part <= 0 || of <= 0 {
        return 0;
    }
    u8::try_from((i128::from(part) * 100 / i128::from(of)).clamp(2, 100)).unwrap_or(100)
}

#[cfg(test)]
mod tests;
