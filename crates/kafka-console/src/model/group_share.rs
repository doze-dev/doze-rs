//! The view-models of a share group's (KIP-932) Lag tab: its share-partitions, in place of committed offsets.

/// One share-partition of a share group: where it starts and ends, and its records in flight by state.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ShareRow {
    pub topic: String,
    pub partition: i32,
    /// The share-partition start offset (SPSO); `None` before its first fetch.
    pub start: Option<i64>,
    /// The share-partition end offset (SPEO).
    pub end: Option<i64>,
    /// Where the log ends.
    pub log_end: Option<i64>,
    /// Records from the start to the log's end not yet done with.
    pub lag: Option<i64>,
    /// The counts are live (fetched since the broker started), not as last stored.
    pub live: bool,
    pub available: u64,
    pub acquired: u64,
    pub acknowledged: u64,
    pub archived: u64,
    /// Records in flight by how many times they have been delivered: (delivery count, records).
    pub deliveries: Vec<(i16, u64)>,
    /// Who holds acquired records: (member, records).
    pub holders: Vec<(String, u64)>,
    /// When the soonest lock runs out, in milliseconds from now.
    pub next_lock_ms: Option<i64>,
}

/// A share group's share-partitions and what they are held to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GroupShareView {
    pub rows: Vec<ShareRow>,
    pub delivery_limit: i64,
    pub lock_ms: i64,
    pub record_locks: i64,
    pub reset: String,
    pub read_committed: bool,
}
