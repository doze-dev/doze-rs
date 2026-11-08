//! What the broker is told at start.

use std::path::PathBuf;

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
    /// Broker-wide values that topics override.
    pub defaults: Defaults,
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
            defaults: Defaults::default(),
        }
    }
}
