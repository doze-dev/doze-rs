//! What the Log page of a partition says: its segments, where cleaning got to, the planner's reading and verdict, what the
//! last round and the last retention pass did, the deletions a round has kept, and how the log is kept on disk. The names
//! start with `Log` so that no other family of pages can meet them.

use super::Cleanup;

/// One segment of the log.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogSegment {
    /// Its first offset, and the offset after its last (where the next segment begins, or the log ends).
    pub base: i64,
    pub next: i64,
    pub bytes: u64,
    /// The newest record's timestamp, else when the file was last written.
    pub newest_ms: Option<i64>,
    pub records: u64,
    /// The offsets compaction removed, as (first, how many).
    pub gaps: Vec<(i64, i64)>,
    /// Removed offsets the survey counted and could not place (a log bigger than its budget).
    pub unplaced: u64,
    /// Every batch of it was read.
    pub exact: bool,
    /// It is the one being written.
    pub active: bool,
}

impl LogSegment {
    /// The offsets it spans.
    #[must_use]
    pub fn span(&self) -> i64 {
        (self.next - self.base).max(0)
    }

    /// The offsets in its gaps.
    #[must_use]
    pub fn removed(&self) -> i64 {
        self.gaps.iter().map(|g| g.1).sum::<i64>() + i64::try_from(self.unplaced).unwrap_or(0)
    }
}

/// A deletion a round kept.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogTombstone {
    /// The key as text (lossy), and whether it was not valid text.
    pub key: String,
    pub binary: bool,
    pub offset: i64,
    pub written_ms: i64,
    pub goes_after_ms: i64,
}

/// What stops cleaning short of the log's end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LogLimit {
    Active,
    Transaction,
    TooNew,
}

/// What the cleaner is doing about the partition.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LogCleaner {
    Disabled,
    Running { since_ms: i64 },
    Failed { at_ms: i64, message: String },
    Watching,
}

/// A round that was put in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogRound {
    pub at_ms: i64,
    pub took_ms: u64,
    pub read: u64,
    pub kept: u64,
    pub replaced: usize,
    pub clean_to: i64,
}

/// What the topic asks of the cleaner.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogRules {
    pub min_dirty_ratio: f64,
    pub min_lag_ms: i64,
    pub max_lag_ms: Option<i64>,
    pub delete_retention_ms: i64,
}

/// What the planner read of the log and what it said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogPlanner {
    pub upto: i64,
    pub limit: LogLimit,
    pub first_dirty: i64,
    pub dirty_start: i64,
    pub dirty_bytes: u64,
    pub clean_bytes: u64,
    pub waited_too_long: bool,
    /// The planner said a round is due now.
    pub due: bool,
}

impl LogPlanner {
    /// The dirty share of the cleanable log, as the planner compares it; `None` with nothing dirty.
    #[must_use]
    pub fn ratio(&self) -> Option<f64> {
        (self.dirty_bytes > 0)
            .then(|| self.dirty_bytes as f64 / (self.dirty_bytes + self.clean_bytes) as f64)
    }
}

/// The compaction side of a log.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogCompaction {
    pub rules: LogRules,
    /// `--log-cleaner-interval`, and when the cleaner last looked.
    pub every_ms: i64,
    pub looked_at_ms: Option<i64>,
    pub planner: LogPlanner,
    pub cleaner: LogCleaner,
    pub last: Option<LogRound>,
}

/// A partition whose start retention moved.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogMoved {
    pub topic: String,
    pub partition: i32,
    pub from: i64,
    pub to: i64,
}

/// The last retention pass.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogPass {
    pub at_ms: i64,
    pub looked: u32,
    pub moved: Vec<LogMoved>,
    pub more: u32,
}

/// The retention side of a log.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogRetentionView {
    pub max_age_ms: Option<i64>,
    pub max_bytes: Option<u64>,
    pub every_ms: i64,
    /// How many of the oldest segments a pass run now would delete.
    pub would_delete: usize,
    pub last_pass: Option<LogPass>,
    pub moved_total: u64,
    pub moved_at_ms: Option<i64>,
}

/// What the last stop left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LogStop {
    None,
    Clean,
    Unclean,
}

/// How the log is kept on disk.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogDurability {
    pub offline: Option<String>,
    pub stop: LogStop,
    pub fsync_ms: i64,
}

/// Where the Log page of another topic is, and what is worth saying of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LogOtherLink {
    /// A compacted log that is not due: "Why not yet?".
    NotDue,
    /// A log that deletes by age or size: "Retention".
    Retention,
    /// A log with nothing in it: "Empty log".
    Empty,
    /// Any other: "Log".
    Plain,
}

/// A line of "Other logs on this broker".
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogOther {
    pub topic: String,
    pub cleanup: Cleanup,
    pub line: String,
    pub sub: Option<String>,
    pub link: LogOtherLink,
}

/// Which of the four boards a log is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LogState {
    /// Nothing was ever written.
    Empty,
    /// Compaction is due now.
    Due,
    /// Compaction is on and the planner says not yet.
    NotDue,
    /// The log only deletes.
    Retention,
}

/// One partition's log, as the page reads it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogView {
    pub topic: String,
    pub partition: i32,
    pub partitions: i32,
    pub now_ms: i64,
    pub cleanup: Cleanup,
    pub start: i64,
    pub end: i64,
    pub last_stable: i64,
    pub never_written: bool,
    pub segments: Vec<LogSegment>,
    pub partial: bool,
    pub segment_bytes: u64,
    pub index_interval_bytes: u64,
    pub segment_ms: Option<i64>,
    pub checkpoint: i64,
    pub tombstones: Vec<LogTombstone>,
    pub compaction: Option<LogCompaction>,
    pub retention: Option<LogRetentionView>,
    pub durability: LogDurability,
    pub others: Vec<LogOther>,
}

impl LogView {
    /// The board this log is in, by its data.
    #[must_use]
    pub fn state(&self) -> LogState {
        if self.never_written
            || (self.end == self.start && self.segments.iter().all(|s| s.bytes == 0))
        {
            return LogState::Empty;
        }
        match (&self.compaction, &self.retention) {
            (Some(c), _) if c.planner.due => LogState::Due,
            (Some(_), _) => LogState::NotDue,
            (None, _) => LogState::Retention,
        }
    }

    /// The records in the log, as it stands: the offsets from the start to the end less the ones compaction removed.
    #[must_use]
    pub fn records(&self) -> i64 {
        let held: i64 = self
            .segments
            .iter()
            .map(|s| i64::try_from(s.records).unwrap_or(0))
            .sum();
        held.max(0)
    }

    /// The bytes of every segment.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.segments.iter().map(|s| s.bytes).sum()
    }
}
