//! The view-model of the Data page (docs/port/data-control.md): what is using the disk and why, and the three ways to give
//! it back.

use super::{Cleanup, Span};

/// What kind of thing a part of the data directory is (in the order a tie is shown).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DataKind {
    /// The partitions of the topics: segments and their indexes.
    Topics,
    /// The metadata store (`meta.redb`): topics, committed offsets, transactions, ACLs, users, schemas.
    Metadata,
    /// Partition directories that belong to no topic.
    Strays,
    /// Everything else (the lock, the record of the last stop).
    Other,
}

/// One kind's share of the data directory.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DataKindRow {
    pub kind: DataKind,
    pub label: String,
    pub bytes: u64,
    /// What is in it, in a line.
    pub note: String,
}

/// The volume the data directory is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VolumeView {
    pub total: u64,
    pub free: u64,
}

/// How loud a heads-up is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AlarmTone {
    /// Worth a look.
    Warn,
    /// The disk is nearly full: writes will start to fail.
    Bad,
}

/// The heads-up Home, the top bar and the Data page show when the disk needs someone.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DiskAlarm {
    pub tone: AlarmTone,
    pub headline: String,
    pub detail: String,
    /// The topic it is about, when it is about one (the page links to its row).
    pub topic: Option<String>,
}

/// One topic on disk.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DataTopicRow {
    pub name: String,
    pub partitions: i32,
    pub bytes: u64,
    /// Its share of the data directory, in thousandths.
    pub share_permille: u32,
    pub segments: usize,
    /// When its oldest data dates from, in milliseconds since the epoch.
    pub oldest_ms: Option<i64>,
    pub last_write_ms: Option<i64>,
    pub cleanup: Cleanup,
    /// What retention keeps, short: `7 days`, `1.0 GiB per partition`, `latest per key`, `none`.
    pub retention: String,
    /// The topic sets its own retention (else the broker's default is in force).
    pub own_retention: bool,
    /// Nothing ever leaves it: no time or size bound, and it does not compact.
    pub unbounded: bool,
    /// Why it holds what it holds, in a sentence.
    pub why: String,
    /// When retention's next pass could take its oldest segment.
    pub next_drop_ms: Option<i64>,
    /// How fast it grew lately, in bytes an hour (negative: it shrank); `None` until the console has looked twice, a minute
    /// apart.
    pub growth_per_hour: Option<i64>,
    /// A transaction is under way on it: emptying it is refused until it ends.
    pub txn_open: bool,
    /// The groups with committed offsets on it (they stay; the next fetch from one is out of range).
    pub read_by: Vec<String>,
}

/// `GET /api/v1/data`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DataView {
    pub now_ms: i64,
    pub data_dir: String,
    /// Every byte of the data directory.
    pub total: u64,
    /// The walk stopped at its bound: every number is at least what it says.
    pub cut: bool,
    pub volume: Option<VolumeView>,
    /// Largest first.
    pub kinds: Vec<DataKindRow>,
    /// Largest first.
    pub topics: Vec<DataTopicRow>,
    /// The bytes of topics the console may not describe (counted, not named).
    pub hidden_bytes: u64,
    /// Directories no topic owns, by name.
    pub strays: Vec<(String, u64)>,
    /// What a topic that sets no retention keeps: the broker's defaults, in a sentence.
    pub defaults: String,
    pub alarm: Option<DiskAlarm>,
    /// What the metadata store holds besides topics.
    pub groups: usize,
    pub transactional_ids: usize,
    /// Subjects in the schema registry; `None` when there is none.
    pub subjects: Option<usize>,
    /// A transaction under way anywhere: Empty all topics and Erase everything are refused until it ends.
    pub txn_under_way: Option<String>,
    /// Since when growth is measured (the oldest look kept), if there is one to measure from.
    pub growth_since_ms: Option<i64>,
}

/// `GET /api/v1/data/empty?topic=`: what Empty topic will remove.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct EmptyTopicView {
    pub topic: String,
    pub partitions: usize,
    pub bytes: u64,
    /// Records below the end of its partitions now (the offsets, gaps that compaction left included).
    pub records: i64,
    pub cleanup: Cleanup,
    /// Who reads it, as the Delete-topic dialog says it; empty when nothing does.
    pub in_use: Vec<Span>,
    /// What happens to everything else, line by line.
    pub keeps: Vec<String>,
    /// Why it cannot be emptied now (a transaction under way), if it cannot.
    pub blocked: Option<String>,
}
