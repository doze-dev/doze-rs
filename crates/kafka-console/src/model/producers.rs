//! What the Producers pages say: the producers a partition remembers with their last batches, the transactions open on the topic
//! and what they hold back, and the aborted transactions the partition remembers. Names start with `Prod` or `Txn` so that
//! no other family of pages can meet them.

/// A batch a producer wrote that the partition still remembers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProdBatch {
    pub first_sequence: i32,
    pub last_sequence: i32,
    pub first_offset: i64,
    pub last_offset: i64,
    pub at_ms: i64,
    /// It is in the producer's open transaction.
    pub open: bool,
}

impl ProdBatch {
    /// How many records the batch held.
    #[must_use]
    pub fn records(&self) -> i64 {
        (self.last_offset - self.first_offset + 1).max(1)
    }
}

/// The transaction a producer has open, as the producers table names it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProdTxn {
    pub id: String,
    pub state: String,
    /// Where it began on this partition, if the partition says.
    pub from: Option<i64>,
}

/// One producer on the partition.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProdRow {
    pub id: i64,
    pub epoch: i16,
    pub last_sequence: Option<i32>,
    pub last_used_ms: i64,
    pub txn: Option<ProdTxn>,
    /// The last batches, newest first.
    pub batches: Vec<ProdBatch>,
    /// The clients the wire recorder saw write with this producer id, as `orders (go, sarama)`, newest first; empty where
    /// there is no recorder or it did not see the producer write.
    pub seen_as: Vec<String>,
}

/// A transaction that was aborted and that the partition still remembers.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProdAborted {
    pub producer: i64,
    pub partition: i32,
    pub first: i64,
    pub marker: i64,
    /// When the marker was written, where the log says.
    pub at_ms: Option<i64>,
}

/// What an offset in the tail of the partition is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ProdCell {
    /// A record a `read_committed` consumer may read.
    Committed,
    /// A commit or an abort marker: a control record that takes an offset.
    Marker,
    /// At or after the last stable offset: held back while a transaction is open.
    Held,
}

/// The last offsets of the partition, one cell each.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProdTail {
    pub from: i64,
    pub cells: Vec<ProdCell>,
}

/// A transaction as ListTransactions and DescribeTransactions say it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TxnRow {
    pub id: String,
    pub state: String,
    pub producer_id: i64,
    pub producer_epoch: i16,
    /// When it began, in milliseconds since the epoch, and its timeout.
    pub since_ms: i64,
    pub timeout_ms: i64,
    /// `(topic, partition)` it has written to.
    pub partitions: Vec<(String, i32)>,
}

impl TxnRow {
    /// Open: the one state in which it holds anything back.
    #[must_use]
    pub fn ongoing(&self) -> bool {
        self.state == "Ongoing"
    }
}

/// The Producers page of one partition.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProducersView {
    pub topic: String,
    pub partitions: i32,
    pub partition: i32,
    pub now_ms: i64,
    pub start: i64,
    pub end: i64,
    pub last_stable: i64,
    pub producers: Vec<ProdRow>,
    /// The transactions open on this topic.
    pub open: Vec<TxnRow>,
    pub aborted: Vec<ProdAborted>,
    pub tail: ProdTail,
    /// The producers the page was asked to open (`?open=`).
    pub opened: Vec<i64>,
}

impl ProducersView {
    /// The records a `read_committed` consumer is held back from on this partition.
    #[must_use]
    pub fn held(&self) -> i64 {
        (self.end - self.last_stable).max(0)
    }
}

/// How much of the topic a transaction holds back on one partition.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TxnPart {
    pub partition: i32,
    /// The offset it began at on this partition, where the partition says.
    pub first: Option<i64>,
    pub start: i64,
    pub end: i64,
    pub last_stable: i64,
}

impl TxnPart {
    /// The records it holds back here: from where it began to the end.
    #[must_use]
    pub fn held(&self) -> i64 {
        self.first
            .map_or(0, |f| (self.end - f.max(self.start)).max(0))
    }
}

/// The page of one transaction (`txn-open`), and what it holds back (`txn-holding`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TxnView {
    pub topic: String,
    pub partitions: i32,
    pub txn: TxnRow,
    pub now_ms: i64,
    /// Its partitions of this topic, with what it holds back on each.
    pub parts: Vec<TxnPart>,
    /// Its partitions of other topics, which are not described here.
    pub elsewhere: Vec<(String, i32)>,
    /// The tail of the first partition it holds back.
    pub tail: ProdTail,
    pub tail_partition: i32,
    /// Every transaction the broker knows.
    pub all: Vec<TxnRow>,
    /// More transactions than the page lists.
    pub more: usize,
}

impl TxnView {
    /// The records the transaction holds back, over this topic's partitions.
    #[must_use]
    pub fn held(&self) -> i64 {
        self.parts.iter().map(TxnPart::held).sum()
    }
}
