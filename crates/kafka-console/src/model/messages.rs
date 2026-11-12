//! The view-model of the Messages page and of one record.

use super::{MessageQuery, TopicHead};

/// Where a record stands for a `read_committed` consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// Below the last stable offset and not aborted: both isolation levels see it.
    Committed,
    /// At or above the last stable offset: a `read_committed` consumer cannot read it yet.
    AboveLso,
    /// Its transaction was aborted: a `read_committed` consumer skips it.
    Aborted,
}

/// What a key or a value is, as far as the bytes say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Not there (a null key, a deletion's value).
    Null,
    /// Zero bytes.
    Empty,
    Text,
    Json,
    /// Not valid UTF-8.
    Binary,
}

/// A key, a value or a header's value, ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub kind: Kind,
    /// The true size in bytes.
    pub bytes: u64,
    /// Text, or the lines of a hex dump; cut at the limit with `clipped` set.
    pub text: String,
    pub clipped: bool,
    /// One line for a table cell.
    pub preview: String,
}

/// One row of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordRow {
    pub partition: i32,
    pub offset: i64,
    pub timestamp_ms: i64,
    pub key: Shown,
    pub value: Shown,
    pub standing: Standing,
}

/// A partition's place in the `read_committed` explanation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    pub partition: i32,
    pub high_watermark: i64,
    pub last_stable: i64,
    /// The transaction that holds it back, if the broker names one.
    pub txn: Option<(String, i64)>,
}

/// What the read did, in numbers a sentence can use.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reach {
    /// Records returned.
    pub returned: usize,
    pub first_offset: Option<i64>,
    pub last_offset: Option<i64>,
    /// The read reached the end of the partition.
    pub reached_end: bool,
    /// Of those returned, how many are at or above the last stable offset.
    pub above_lso: usize,
    /// A key or time search: how many records it looked at, and whether it looked at all of them.
    pub scanned: Option<(u64, bool)>,
    /// Whether the table is cut at the limit (more matched).
    pub more: bool,
}

/// The Messages page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessagesView {
    pub head: TopicHead,
    pub query: MessageQuery,
    /// A sentence for each filter value that could not be used.
    pub notes: Vec<String>,
    /// Newest first.
    pub rows: Vec<RecordRow>,
    pub reach: Reach,
    pub holds: Vec<Hold>,
}

/// A header of a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderRow {
    pub key: String,
    pub value: Shown,
}

/// The transaction a record belongs to, as far as the broker says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordTxn {
    pub id: Option<String>,
    /// `Ongoing`, `PrepareCommit`, ... where the coordinator still has it.
    pub state: Option<String>,
    pub since_ms: Option<i64>,
    pub timeout_ms: Option<i64>,
}

/// One record, completely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordView {
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub last_stable: i64,
    pub key: Shown,
    pub value: Shown,
    pub headers: Vec<HeaderRow>,
    pub timestamp_ms: i64,
    /// The broker stamped the time (LogAppendTime) and not the producer.
    pub log_append_time: bool,
    pub codec: String,
    /// Producer id, epoch and the record's sequence number; none for a plain producer.
    pub producer: Option<(i64, i16, i64)>,
    pub transactional: bool,
    pub txn: Option<RecordTxn>,
    pub standing: Standing,
}
