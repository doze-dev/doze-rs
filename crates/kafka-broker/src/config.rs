//! What the broker is told at start.

use std::path::PathBuf;
use std::time::Duration;

/// The broker-wide values that a topic's own config overrides. Retention uses Kafka's way of saying "no limit":
/// -1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Defaults {
    /// Roll to a new segment at this size (`segment.bytes`).
    pub segment_bytes: i64,
    /// One index entry per this many bytes (`index.interval.bytes`).
    pub index_interval_bytes: i64,
    /// The largest record batch accepted (`max.message.bytes`).
    pub message_max_bytes: i64,
    /// Delete the oldest segments once a partition exceeds this (`retention.bytes`).
    pub retention_bytes: i64,
    /// Delete segments older than this (`retention.ms`).
    pub retention_ms: i64,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            segment_bytes: 128 << 20,
            index_interval_bytes: 4 << 10,
            message_max_bytes: 1_048_588,
            retention_bytes: -1,
            retention_ms: -1,
        }
    }
}

/// The broker's configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Where everything is kept: the metadata database and a directory per partition.
    pub data_dir: PathBuf,
    /// This broker's id, the only one in the cluster.
    pub node_id: i32,
    /// The cluster id reported in Metadata.
    pub cluster_id: String,
    /// Whether a Metadata request may create the topics it asks for.
    pub auto_create_topics: bool,
    /// Partitions of an auto-created topic, and of a CreateTopics that asks for the default.
    pub default_partitions: i32,
    /// The most partitions one topic may have: each is a directory and, in use, open files.
    pub max_partitions: i32,
    /// How many partitions stay open at once (each holds files open): past it the least recently used are closed,
    /// and open again when asked for. Zero keeps every partition open once opened.
    pub max_open_partitions: usize,
    /// Broker-wide values that topics override.
    pub defaults: Defaults,
    /// Appends that did not ask to be durable (`acks=1`) are flushed this long after the first of them. Zero
    /// flushes each group at once.
    pub fsync_interval: Duration,
    /// How long the first rebalance of an empty consumer group waits for more members to arrive, so that members
    /// starting together share a generation. Zero: not at all.
    pub group_initial_rebalance_delay: Duration,
    /// How long committed offsets are kept after their group has gone, or after they were committed if it never had
    /// members. Zero: for ever.
    pub offsets_retention: Duration,
    /// How often the offsets are looked at; `None`: a hundredth of the retention, between 50 ms and ten minutes.
    pub offsets_check_every: Option<Duration>,
    /// How long a member of a consumer group of the new protocol may be silent before it is removed.
    pub consumer_session_timeout: Duration,
    /// The least time between two computations of a consumer group's target assignment (Kafka's
    /// `group.consumer.assignment.interval.ms`).
    pub consumer_assignment_interval: Duration,
}

impl Config {
    /// The defaults, keeping data under `data_dir`.
    #[must_use]
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            node_id: 1,
            cluster_id: "doze-kafka".to_owned(),
            auto_create_topics: true,
            default_partitions: 1,
            max_partitions: 1000,
            max_open_partitions: 512,
            defaults: Defaults::default(),
            fsync_interval: Duration::from_millis(500),
            group_initial_rebalance_delay: Duration::from_millis(200),
            offsets_retention: Duration::from_secs(7 * 24 * 3600),
            offsets_check_every: None,
            consumer_session_timeout: Duration::from_secs(45),
            consumer_assignment_interval: Duration::from_secs(1),
        }
    }
}
