//! What every page of one topic says about it: its shape, its bounds, what holds it back.

use super::Cleanup;

/// One partition's offsets and files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionRow {
    pub partition: i32,
    /// The log start: the oldest offset held.
    pub start: i64,
    /// The log end: the next offset to be written (the high watermark, with nothing replicated).
    pub end: i64,
    /// The last stable offset: where a `read_committed` consumer stops.
    pub last_stable: i64,
    pub bytes: Option<u64>,
    pub last_write_ms: Option<i64>,
    /// Why writes are refused, once a flush has failed.
    pub offline: Option<String>,
}

impl PartitionRow {
    /// The records a `read_committed` consumer is held back from.
    #[must_use]
    pub fn held(&self) -> i64 {
        (self.end - self.last_stable).max(0)
    }

    #[must_use]
    pub fn records(&self) -> i64 {
        (self.end - self.start).max(0)
    }
}

/// A transaction that is open on the topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTxn {
    pub id: String,
    pub producer_id: i64,
    pub producer_epoch: i16,
    pub since_ms: i64,
    pub timeout_ms: i64,
    /// The partitions of this topic it has written to, and the offset it began at on each, where the broker says.
    pub partitions: Vec<(i32, Option<i64>)>,
}

/// The tabs' counts, where the broker can say them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TabCounts {
    /// Groups with offsets committed on the topic.
    pub consumers: usize,
    /// Producers active on its partitions.
    pub producers: usize,
}

/// The versions of the requests the pages say they make (the highest the broker's profile offers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CallVersions {
    pub fetch: i16,
    pub list_offsets: i16,
}

/// One topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicHead {
    pub name: String,
    pub cleanup: Cleanup,
    /// `retention.ms` in force, if the broker said (`-1` is for ever).
    pub retention_ms: Option<i64>,
    /// The partitions, in order.
    pub parts: Vec<PartitionRow>,
    pub txns: Vec<OpenTxn>,
    pub tabs: TabCounts,
    pub calls: CallVersions,
    pub now_ms: i64,
}

impl TopicHead {
    #[must_use]
    pub fn records(&self) -> i64 {
        self.parts.iter().map(PartitionRow::records).sum()
    }

    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.parts.iter().filter_map(|p| p.bytes).sum()
    }

    #[must_use]
    pub fn last_write_ms(&self) -> Option<i64> {
        self.parts.iter().filter_map(|p| p.last_write_ms).max()
    }

    /// No record was ever written: every partition starts and ends at 0.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parts.iter().all(|p| p.end == 0 && p.start == 0)
    }

    /// The partitions whose writes are refused.
    #[must_use]
    pub fn offline(&self) -> Vec<&PartitionRow> {
        self.parts.iter().filter(|p| p.offline.is_some()).collect()
    }

    /// Records held back from `read_committed` consumers, over all partitions.
    #[must_use]
    pub fn held(&self) -> i64 {
        self.parts.iter().map(PartitionRow::held).sum()
    }
}
