//! What is using the disk: the data directory walked by kind, each topic's share and what its configs keep.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use kafka_auth::authz::Operation;
use kafka_meta::Topic;
use kafka_storage::fs::{Fs, Space};
use kafka_storage::oldest_on_disk;
use kafka_storage::topic_dirs::partition_dir;
use kafka_storage::usage::{PartitionDir, walk_data_dir};
use kafka_wire::ids::PartitionId;

use crate::{Broker, Caller, Defaults, topic_config};

/// What a topic's configs keep of what is written to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keeps {
    /// `cleanup.policy` names `compact`: the latest value of each key is kept.
    pub compact: bool,
    /// `cleanup.policy` names `delete`: retention applies.
    pub delete: bool,
    /// `retention.ms` in force (the topic's own, else the broker's); `None` is no bound, and so is any value on a topic that
    /// only compacts (retention does not touch it).
    pub max_age_ms: Option<i64>,
    /// `retention.bytes` in force, per partition.
    pub max_bytes: Option<u64>,
    /// The topic sets its own `retention.ms` or `retention.bytes` (else the broker's defaults are what is in force).
    pub own: bool,
}

impl Keeps {
    /// What `own` (a topic's configs) keeps, with `defaults` for what it does not set.
    #[must_use]
    pub(crate) fn of(defaults: &Defaults, own: &BTreeMap<String, String>) -> Self {
        let compact = topic_config::compacts(own);
        let delete = !topic_config::compacts_only(own);
        let retention = topic_config::retention(defaults, own);
        Self {
            compact,
            delete,
            max_age_ms: retention.max_age_ms.filter(|_| delete),
            max_bytes: retention.max_bytes.filter(|_| delete),
            own: own.contains_key("retention.ms") || own.contains_key("retention.bytes"),
        }
    }

    /// Nothing ever leaves: it deletes (does not compact) and has no time or size bound. Such a topic grows for as long as
    /// anything writes to it.
    #[must_use]
    pub fn unbounded(&self) -> bool {
        !self.compact && self.max_age_ms.is_none() && self.max_bytes.is_none()
    }
}

/// One topic on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicUsage {
    pub name: String,
    pub partitions: i32,
    /// Every file of its partitions: segments, their indexes, the small records beside them.
    pub bytes: u64,
    pub segments: usize,
    /// When its oldest data on disk dates from (the first batch of each partition's first segment), if any has a time.
    pub oldest_ms: Option<i64>,
    /// When it was last written (its newest segment), in milliseconds since the epoch.
    pub last_write_ms: Option<i64>,
    pub keeps: Keeps,
    /// When retention's next pass could take its oldest segment, as the last pass worked out (and nothing has changed since).
    pub next_drop_ms: Option<i64>,
}

/// A partition directory that belongs to no topic the broker has (a metadata store that was lost or replaced leaves them).
/// A topic of the same name made later clears them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stray {
    /// The directory's name.
    pub name: String,
    pub bytes: u64,
}

/// The data directory, by kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataUsage {
    pub data_dir: PathBuf,
    /// The volume the data directory is on: its size and its room. `None` where the platform does not say.
    pub space: Option<Space>,
    /// The topics the caller may describe, largest first.
    pub topics: Vec<TopicUsage>,
    /// The bytes of topics the caller may not describe (counted, not named).
    pub hidden_bytes: u64,
    pub strays: Vec<Stray>,
    /// The metadata store: topics, committed offsets, transactions, ACLs, users and schemas.
    pub meta_bytes: u64,
    /// Everything else (the lock, the record of the last stop, files nobody here made).
    pub other_bytes: u64,
    pub other_files: u64,
    /// The walk stopped at its bound: every number is at least what it says.
    pub cut: bool,
    /// Groups with committed offsets, and transactional ids the coordinator remembers: what the metadata store holds.
    pub groups: usize,
    pub transactional_ids: usize,
    /// What a topic that sets no retention of its own keeps: the broker's defaults.
    pub defaults: Keeps,
}

impl DataUsage {
    /// The bytes of every topic, named or not.
    #[must_use]
    pub fn topics_bytes(&self) -> u64 {
        self.topics.iter().map(|t| t.bytes).sum::<u64>() + self.hidden_bytes
    }

    /// The bytes of the partition directories no topic owns.
    #[must_use]
    pub fn stray_bytes(&self) -> u64 {
        self.strays.iter().map(|s| s.bytes).sum()
    }

    /// Every byte of the data directory.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.topics_bytes() + self.stray_bytes() + self.meta_bytes + self.other_bytes
    }
}

impl<F: Fs> Broker<F> {
    /// What is using the data directory, if `caller` may DESCRIBE the cluster; topics it may not DESCRIBE are counted and not
    /// named. Reads the disk (a walk of the directory, bounded, and one batch header per partition), so call it off the async
    /// threads. Nothing is opened for writing and nothing is kept.
    #[must_use]
    pub fn data_usage(&self, caller: &Caller) -> Option<DataUsage> {
        if !self.cluster_allows(caller, Operation::Describe) {
            return None;
        }
        let data = &self.config.data_dir;
        let walked = walk_data_dir(&self.fs, data).unwrap_or_default();
        let defaults = self.defaults.get();
        let mut out = DataUsage {
            data_dir: data.clone(),
            space: self.fs.space(data).ok().flatten(),
            topics: Vec::new(),
            hidden_bytes: 0,
            strays: Vec::new(),
            meta_bytes: walked.meta_bytes,
            other_bytes: walked.other_bytes,
            other_files: walked.other_files,
            cut: walked.cut,
            groups: self.meta.groups_with_offsets().map_or(0, |g| g.len()),
            transactional_ids: self.txns.all().len(),
            defaults: Keeps::of(&defaults, &BTreeMap::new()),
        };
        let mut topics: BTreeMap<String, (TopicUsage, Arc<Topic>, bool)> = self
            .meta
            .topics()
            .into_iter()
            .map(|t| {
                let row = TopicUsage {
                    name: t.name.to_string(),
                    partitions: t.partitions.get(),
                    bytes: 0,
                    segments: 0,
                    oldest_ms: None,
                    last_write_ms: None,
                    keeps: Keeps::of(&defaults, &t.config),
                    next_drop_ms: None,
                };
                let seen = self.topic_allows(caller, t.name.as_str(), Operation::Describe);
                (t.name.to_string(), (row, t, seen))
            })
            .collect();
        for dir in walked.partitions {
            match topics.get_mut(&dir.topic) {
                Some((row, topic, _)) if dir.partition < topic.partitions.get() => {
                    self.add_partition(row, topic, &dir);
                }
                _ => out.strays.push(Stray {
                    name: format!("{}-{}", dir.topic, dir.partition),
                    bytes: dir.bytes,
                }),
            }
        }
        for (row, _, seen) in topics.into_values() {
            if seen {
                out.topics.push(row);
            } else {
                out.hidden_bytes += row.bytes;
            }
        }
        out.topics
            .sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
        Some(out)
    }

    /// Counts one partition directory into its topic's row.
    fn add_partition(&self, row: &mut TopicUsage, topic: &Topic, dir: &PartitionDir) {
        row.bytes += dir.bytes;
        row.segments += dir.segments;
        row.last_write_ms = row.last_write_ms.max(dir.last_write_ms);
        let Some(id) = PartitionId::new(dir.partition) else {
            return;
        };
        if dir.segments > 0 {
            let path = partition_dir(&self.config.data_dir, &topic.name, id);
            if let Some(ms) = oldest_on_disk(&self.fs, &path) {
                row.oldest_ms = Some(row.oldest_ms.map_or(ms, |o| o.min(ms)));
            }
        }
        if let Some(due) = self.partitions.retention_due(topic.id, id) {
            row.next_drop_ms = Some(row.next_drop_ms.map_or(due, |d| d.min(due)));
        }
    }
}
