//! The view-model of the topics list, and the facts about one topic that its pages share.

/// What a topic does with old records (`cleanup.policy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cleanup {
    Delete,
    Compact,
    /// `compact,delete`: both.
    Both,
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicRow {
    pub name: String,
    /// Kafka's own topic (shown only when asked for).
    pub internal: bool,
    pub partitions: i32,
    pub cleanup: Cleanup,
    /// A transaction is open on one of its partitions.
    pub txn_open: bool,
    /// Nothing has been written to it (no bytes on disk).
    pub empty: bool,
    /// `retention.ms` where the topic sets its own (`-1` is for ever).
    pub retention_ms: Option<i64>,
    pub segments: Option<usize>,
    /// The groups that have committed offsets on it.
    pub read_by: Vec<String>,
    pub bytes: Option<u64>,
    /// When it was last written (the newest segment file), in milliseconds since the epoch.
    pub last_write_ms: Option<i64>,
    /// How many of its partitions are offline.
    pub offline: usize,
}

impl TopicRow {
    /// Kafka's own `__consumer_offsets`, which Metadata never lists: the console knows its name and how many slots of the
    /// group coordinator it stands for (REALITY: the broker keeps commits in its own store, so there is no log behind it).
    #[must_use]
    pub fn internal() -> Self {
        Self {
            name: "__consumer_offsets".to_owned(),
            internal: true,
            partitions: 50,
            cleanup: Cleanup::Compact,
            txn_open: false,
            empty: false,
            retention_ms: None,
            segments: None,
            read_by: Vec::new(),
            bytes: None,
            last_write_ms: None,
            offline: 0,
        }
    }
}

/// Which page of the list the data calls for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicsState {
    /// The broker holds no topic (`topics-none`).
    None,
    /// A name filter matches nothing (`topics-filtered`).
    Filtered,
    /// Topics to show (`topics-list`).
    List,
}

/// The topics list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicsView {
    /// The rows after the filter, by name.
    pub rows: Vec<TopicRow>,
    /// How many topics there are (internal ones apart).
    pub total: usize,
    /// How many partitions they have between them.
    pub partitions: usize,
    /// What the name filter says (`?q=`).
    pub filter: String,
    pub show_internal: bool,
    /// How many groups the broker knows (the other tab).
    pub groups: usize,
    /// The topic written to last and when.
    pub newest: Option<(String, i64)>,
    pub now_ms: i64,
    /// More topics matched than the page shows.
    pub truncated: bool,
}

impl TopicsView {
    #[must_use]
    pub fn state(&self) -> TopicsState {
        if self.total == 0 {
            TopicsState::None
        } else if self.rows.iter().all(|r| r.internal) && !self.filter.is_empty() {
            TopicsState::Filtered
        } else {
            TopicsState::List
        }
    }
}
