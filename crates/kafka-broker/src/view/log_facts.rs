//! The log of one partition as the Log page says it: its segments, where cleaning got to, what the cleaner's planner says of
//! it (judged by the planner itself, [`kafka_storage::compact::judge`], on the facts it would be given), what the last round
//! and the last retention pass did, the deletions a round has kept, and how the log is kept on disk.
//!
//! Lazy: every call reads the files and builds an owned answer; nothing is kept here. The partition is opened if it must be
//! (as a Fetch would open it) and let go again if nothing else holds it, so that looking leaves the broker as it was.

use kafka_auth::authz::operation;
use kafka_storage::compact::{Plan, Reading, judge, reading};
use kafka_storage::fs::Fs;
use kafka_storage::topic_dirs::partition_dir;
use kafka_storage::{Budget, SegmentWalk, Tombstone};
use kafka_wire::ids::{PartitionId, TopicName};

use super::{ConsoleView, LastStop};
use crate::log_notes::{CleanerNote, PassNote, RoundNote};
use crate::{Caller, clock, topic_config};

/// What a topic's `cleanup.policy` asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogPolicy {
    pub compact: bool,
    pub delete: bool,
}

/// What the cleaner is doing about the partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanerState {
    /// `--log-cleaner-enable false`: no log is ever cleaned.
    Disabled,
    /// A round is running, begun at this time.
    Running { since_ms: i64 },
    /// The last round failed, at this time, in the broker's words.
    Failed { at_ms: i64, message: String },
    /// Looking now and then.
    Watching,
}

/// The planner's side of a log whose topic compacts.
#[derive(Debug, Clone, PartialEq)]
pub struct Compaction {
    pub min_dirty_ratio: f64,
    pub min_lag_ms: i64,
    /// `None`: not set, so only the share decides.
    pub max_lag_ms: Option<i64>,
    pub delete_retention_ms: i64,
    /// `--log-cleaner-interval`, and when the cleaner last looked (never since the broker began: `None`).
    pub every_ms: i64,
    pub looked_at_ms: Option<i64>,
    /// What the planner read of the log, and its verdict: `Some` is "a round would be run now".
    pub reading: Reading,
    pub verdict: Option<Plan>,
    pub state: CleanerState,
    /// The last round of this partition put in place (kept in memory, forgotten when the broker stops).
    pub last: Option<RoundNote>,
}

/// Retention's side of a log whose topic deletes.
#[derive(Debug, Clone, PartialEq)]
pub struct LogRetention {
    pub max_age_ms: Option<i64>,
    pub max_bytes: Option<u64>,
    /// `--log-retention-check-interval`.
    pub every_ms: i64,
    /// How many of the oldest segments a pass run now would delete (the broker's own rule, [`kafka_storage::count_to_drop`]).
    pub would_delete: usize,
    /// The last pass over every topic, and how many partitions' starts retention has moved since the broker began.
    pub last_pass: Option<PassNote>,
    pub moved_total: u64,
    pub moved_at_ms: Option<i64>,
}

/// How the partition's log is kept on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Durability {
    /// Why writes are refused: the first failed flush. `None` while it is online.
    pub offline: Option<String>,
    pub last_stop: LastStop,
    /// `--fsync-interval`: appends that did not ask to be durable are flushed this long after the first.
    pub fsync_ms: i64,
}

/// One partition's log.
#[derive(Debug, Clone, PartialEq)]
pub struct LogFacts {
    pub topic: String,
    pub partition: i32,
    /// How many partitions the topic has.
    pub partitions: i32,
    pub now_ms: i64,
    pub policy: LogPolicy,
    /// The log start, the log end (the high watermark, with one broker) and the last stable offset.
    pub start: i64,
    pub end: i64,
    pub last_stable: i64,
    /// Nothing was ever written: no files are open and none are needed.
    pub never_written: bool,
    pub segments: Vec<SegmentWalk>,
    /// Some segment was read in part (a very large log): what is listed is true, what is missing is said.
    pub partial: bool,
    /// `segment.bytes`, `index.interval.bytes` and `segment.ms` of the topic.
    pub segment_bytes: u64,
    pub index_interval_bytes: u64,
    pub segment_ms: Option<i64>,
    /// Where cleaning last got to (`cleaner-checkpoint`), as the planner reads it.
    pub checkpoint: i64,
    /// The deletions a round has kept, by the time they go.
    pub tombstones: Vec<Tombstone>,
    pub compaction: Option<Compaction>,
    pub retention: Option<LogRetention>,
    pub durability: Durability,
}

impl<F: Fs> ConsoleView<'_, F> {
    /// The newest round of cleaning that was put in place on a topic that still exists and that `caller` may DESCRIBE: the
    /// topic, the partition and the round. Kept in memory and forgotten when the broker stops, so a broker that has cleaned
    /// nothing since it began has none.
    #[must_use]
    pub fn last_round(&self, caller: &Caller) -> Option<(String, i32, RoundNote)> {
        let b = self.broker;
        b.log_notes
            .rounds_newest_first()
            .into_iter()
            .find_map(|(id, partition, round)| {
                let topic = b.meta.topic_by_id(id)?;
                b.topic_allows(caller, topic.name.as_str(), operation::DESCRIBE)
                    .then(|| (topic.name.as_str().to_owned(), partition, round))
            })
    }

    /// The log of `partition` of `topic`, if `caller` may DESCRIBE the topic and the partition exists; `None` otherwise (the
    /// two are not told apart, as Kafka does not).
    pub async fn log_facts(
        &self,
        caller: &Caller,
        topic: &str,
        partition: i32,
    ) -> Option<LogFacts> {
        let b = self.broker;
        if !b.topic_allows(caller, topic, operation::DESCRIBE) {
            return None;
        }
        let found = b.meta.topic(&TopicName::parse(topic).ok()?)?;
        let id = PartitionId::new(partition).filter(|p| p.get() < found.partitions.get())?;
        let was_open = b.partitions.is_open(found.id, id);
        let open = b.partitions.get_unless_empty(&found, id).await.ok()?;
        let mut facts = self.empty(&found, partition);
        if let Some(p) = &open {
            let reader = p.reader();
            let walk = crate::blocking::run(|| reader.survey(Budget::default()));
            facts.start = walk.start;
            facts.end = walk.end;
            facts.last_stable = p.stability().last_stable.get();
            facts.partial = walk.partial;
            facts.tombstones = walk.tombstones.clone();
            facts.durability.offline = reader.offline_cause();
            let dir = partition_dir(&b.config.data_dir, &found.name, id);
            facts.checkpoint = self.checkpoint(&dir, &walk);
            facts.compaction = self.compaction(&found, &walk, &facts, id);
            facts.retention = self.retention(&found, &walk, &facts);
            facts.segments = walk.segments;
            facts.never_written = false;
        }
        drop(open);
        if !was_open {
            b.partitions.release_if_idle(found.id, id).await;
        }
        Some(facts)
    }

    fn empty(&self, topic: &kafka_meta::Topic, partition: i32) -> LogFacts {
        let b = self.broker;
        let defaults = b.defaults.get();
        let config = topic_config::log_config(&defaults, &topic.config);
        let kinds: Vec<String> = topic.config.get("cleanup.policy").map_or_else(
            || vec!["delete".to_owned()],
            |p| {
                p.split(',')
                    .map(|k| k.trim().to_ascii_lowercase())
                    .collect()
            },
        );
        LogFacts {
            topic: topic.name.to_string(),
            partition,
            partitions: topic.partitions.get(),
            now_ms: clock::now().get(),
            policy: LogPolicy {
                compact: kinds.iter().any(|k| k == "compact"),
                delete: kinds.iter().any(|k| k == "delete") || kinds.is_empty(),
            },
            start: 0,
            end: 0,
            last_stable: 0,
            never_written: true,
            segments: Vec::new(),
            partial: false,
            segment_bytes: config.segment_bytes,
            index_interval_bytes: config.index_interval_bytes,
            segment_ms: config.segment_ms,
            checkpoint: 0,
            tombstones: Vec::new(),
            compaction: None,
            retention: None,
            durability: Durability {
                offline: None,
                last_stop: self.last_stop(),
                fsync_ms: i64::try_from(b.config.fsync_interval.as_millis()).unwrap_or(i64::MAX),
            },
        }
    }

    fn last_stop(&self) -> LastStop {
        let b = self.broker;
        if b.first_start {
            LastStop::None
        } else if b.stop_was_clean() {
            LastStop::Clean
        } else {
            LastStop::Unclean
        }
    }

    /// Where the cleaner would go on from: as `PartitionLog::clean_job` reads it, a checkpoint past the log's end is no place.
    fn checkpoint(&self, dir: &std::path::Path, walk: &kafka_storage::Walk) -> i64 {
        let recorded = kafka_storage::compact::checkpoint_of(&self.broker.fs, dir);
        if recorded > walk.end {
            walk.start
        } else {
            recorded.max(walk.start)
        }
    }

    fn compaction(
        &self,
        topic: &kafka_meta::Topic,
        walk: &kafka_storage::Walk,
        facts: &LogFacts,
        id: PartitionId,
    ) -> Option<Compaction> {
        let b = self.broker;
        if !topic_config::compacts(&topic.config) {
            return None;
        }
        let params = topic_config::cleaner(
            &b.defaults.get(),
            &topic.config,
            facts.now_ms,
            b.config.cleaner_map_bytes,
        );
        let (sealed, active_base) = walk.planner_facts();
        let read = reading(
            &sealed,
            facts.checkpoint,
            facts.last_stable,
            active_base,
            &params,
        );
        let verdict = judge(
            &sealed,
            facts.checkpoint,
            facts.last_stable,
            active_base,
            &params,
        );
        let note: CleanerNote = b
            .log_notes
            .cleaner_of(topic.id, id.get())
            .unwrap_or_default();
        let state = if !b.config.log_cleaner_enable {
            CleanerState::Disabled
        } else if let Some(since_ms) = note.running_since_ms {
            CleanerState::Running { since_ms }
        } else if let Some((at_ms, message)) = note.failed {
            CleanerState::Failed { at_ms, message }
        } else {
            CleanerState::Watching
        };
        Some(Compaction {
            min_dirty_ratio: params.min_dirty_ratio,
            min_lag_ms: params.min_lag_ms,
            max_lag_ms: (params.max_lag_ms != i64::MAX).then_some(params.max_lag_ms),
            delete_retention_ms: params.delete_retention_ms,
            every_ms: i64::try_from(b.config.log_cleaner_every.as_millis()).unwrap_or(i64::MAX),
            looked_at_ms: b.log_notes.looked_at(),
            reading: read,
            verdict,
            state,
            last: note.last,
        })
    }

    fn retention(
        &self,
        topic: &kafka_meta::Topic,
        walk: &kafka_storage::Walk,
        facts: &LogFacts,
    ) -> Option<LogRetention> {
        let b = self.broker;
        if !facts.policy.delete || topic_config::compacts_only(&topic.config) {
            return None;
        }
        let policy = topic_config::retention(&b.defaults.get(), &topic.config);
        let (moved_total, moved_at_ms) = b.log_notes.moved();
        let would_delete = would_delete(walk, policy, facts.now_ms);
        Some(LogRetention {
            max_age_ms: policy.max_age_ms,
            max_bytes: policy.max_bytes,
            every_ms: i64::try_from(b.config.log_retention_check_every.as_millis())
                .unwrap_or(i64::MAX),
            would_delete,
            last_pass: b.log_notes.last_pass(),
            moved_total,
            moved_at_ms,
        })
    }
}

/// How many of the oldest segments a pass at `now_ms` would delete: the rule `PartitionLog::enforce_retention` applies, on the
/// sizes and ages the walk read (an empty active segment is never deleted; a segment with no record timestamp is as old as
/// its last write).
fn would_delete(
    walk: &kafka_storage::Walk,
    policy: kafka_storage::Retention,
    now_ms: i64,
) -> usize {
    if !policy.is_set() {
        return 0;
    }
    let mut kept: Vec<&SegmentWalk> = walk.segments.iter().collect();
    if kept.last().is_some_and(|s| s.bytes == 0) {
        kept.pop();
    }
    let sizes: Vec<u64> = kept.iter().map(|s| s.bytes).collect();
    let age = |i: usize| {
        Ok(kept
            .get(i)
            .map_or(0, |s| s.newest_ms.or(s.modified_ms).unwrap_or(0)))
    };
    kafka_storage::count_to_drop(
        &sizes,
        age,
        policy,
        kafka_wire::ids::TimestampMs::from_wire(now_ms),
    )
    .unwrap_or(0)
}
