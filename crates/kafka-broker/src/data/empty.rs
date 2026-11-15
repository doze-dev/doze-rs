//! Empty a topic, or every topic: every record gone and its disk freed now. The topic, its partitions, its configs and the
//! groups' committed offsets stay; each partition carries on from where it ended.
//!
//! A consumer below the new start is told OFFSET_OUT_OF_RANGE, as after retention, and resets by its `auto.offset.reset`
//! (earliest and latest are the same offset now). A producer carries on: its next batch gets the next offset. A fetch that
//! waits at the end goes on waiting there.

use std::sync::Arc;

use kafka_auth::authz::Operation;
use kafka_meta::{Topic, TxnState};
use kafka_storage::fs::Fs;
use kafka_storage::topic_dirs::partition_dir;
use kafka_storage::usage::dir_bytes;
use kafka_wire::ids::{PartitionId, TopicName};

use super::DataRefusal;
use crate::partitions::Emptying;
use crate::{Broker, Caller, blocking};

/// What emptying a topic did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Emptied {
    pub topic: String,
    /// The partitions that held anything (one never written to is left as it is: there is nothing to free).
    pub partitions: usize,
    /// How many offsets were below the end, and are gone: the records, and the gaps compaction had left.
    pub offsets: i64,
    /// The bytes the partitions' directories no longer hold.
    pub freed_bytes: u64,
}

/// What emptying every topic did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmptiedAll {
    /// Each topic, in name order.
    pub topics: Vec<Emptied>,
}

impl EmptiedAll {
    #[must_use]
    pub fn freed_bytes(&self) -> u64 {
        self.topics.iter().map(|t| t.freed_bytes).sum()
    }

    #[must_use]
    pub fn offsets(&self) -> i64 {
        self.topics.iter().map(|t| t.offsets).sum()
    }
}

impl<F: Fs> Broker<F> {
    /// Empties one topic. The caller must be allowed to DELETE it (as DeleteRecords asks); one that may not is refused
    /// before the topic is looked for, so that a topic it may not see stays hidden.
    ///
    /// # Errors
    /// [`DataRefusal`]: no right, no such topic, a transaction under way on it, or a partition that could not be emptied.
    pub async fn empty_topic(&self, caller: &Caller, name: &str) -> Result<Emptied, DataRefusal> {
        if !self.topic_allows(caller, name, Operation::Delete) {
            return Err(DataRefusal::Denied {
                right: format!("DELETE on topic {name}"),
                cluster: false,
            });
        }
        let topic = TopicName::parse(name)
            .ok()
            .and_then(|n| self.meta.topic(&n))
            .ok_or_else(|| DataRefusal::NoTopic {
                topic: name.to_owned(),
            })?;
        if let Some(id) = self.txn_under_way(Some(name)).await {
            return Err(DataRefusal::TxnOpen {
                topic: Some(name.to_owned()),
                id: Some(id),
            });
        }
        self.empty(&topic).await
    }

    /// Empties every topic, in name order. The caller must be allowed to ALTER the cluster. Refused, before anything is
    /// touched, while any transaction is under way.
    ///
    /// # Errors
    /// [`DataRefusal`]; a partition that fails stops the run, and the topics before it stay emptied.
    pub async fn empty_topics(&self, caller: &Caller) -> Result<EmptiedAll, DataRefusal> {
        if !self.cluster_allows(caller, Operation::Alter) {
            return Err(DataRefusal::Denied {
                right: "ALTER on the cluster".to_owned(),
                cluster: true,
            });
        }
        if let Some(id) = self.txn_under_way(None).await {
            return Err(DataRefusal::TxnOpen {
                topic: None,
                id: Some(id),
            });
        }
        let mut topics: Vec<Arc<Topic>> = self.meta.topics();
        topics.sort_by(|a, b| a.name.cmp(&b.name));
        let mut out = EmptiedAll::default();
        for topic in topics {
            out.topics.push(self.empty(&topic).await?);
        }
        Ok(out)
    }

    /// Empties each partition of `topic` that holds anything, through its writer. A partition this opened is let go again.
    async fn empty(&self, topic: &Topic) -> Result<Emptied, DataRefusal> {
        let mut out = Emptied {
            topic: topic.name.to_string(),
            ..Emptied::default()
        };
        for id in topic.partitions.ids() {
            let freed = self.empty_one(topic, id).await?;
            if let Some((offsets, bytes)) = freed {
                out.partitions += 1;
                out.offsets += offsets;
                out.freed_bytes += bytes;
            }
        }
        tracing::info!(topic = %out.topic, offsets = out.offsets, freed_bytes = out.freed_bytes, "emptied a topic");
        Ok(out)
    }

    /// One partition: the offsets it lost and the bytes it freed, or `None` when it held nothing.
    async fn empty_one(
        &self,
        topic: &Topic,
        id: PartitionId,
    ) -> Result<Option<(i64, u64)>, DataRefusal> {
        let failed = |message: String| DataRefusal::Storage {
            what: format!("partition {id} of {}", topic.name),
            message,
        };
        let dir = partition_dir(&self.config.data_dir, &topic.name, id);
        let before = blocking::run(|| dir_bytes(&self.fs, &dir));
        let was_open = self.partitions.is_open(topic.id, id);
        let partition = match self.partitions.get_unless_empty(topic, id).await {
            Ok(Some(p)) => p,
            Ok(None) => return Ok(None),
            Err(e) => return Err(failed(e.to_string())),
        };
        let done = partition.empty().await;
        drop(partition);
        if !was_open {
            self.partitions.release_if_idle(topic.id, id).await;
        }
        let offsets = match done {
            Ok(Emptying::Emptied { offsets }) => offsets,
            Ok(Emptying::TxnOpen) => {
                return Err(DataRefusal::TxnOpen {
                    topic: Some(topic.name.to_string()),
                    id: None,
                });
            }
            Err(e) => return Err(failed(e.to_string())),
        };
        let after = blocking::run(|| dir_bytes(&self.fs, &dir));
        Ok(Some((offsets, before.saturating_sub(after))))
    }

    /// The transactional id of a transaction under way (begun, not yet finished) that writes to `topic`, or to any topic.
    pub(super) async fn txn_under_way(&self, topic: Option<&str>) -> Option<String> {
        for entry in self.txns.all() {
            let e = entry.read().await;
            let busy = matches!(
                e.rec.state,
                TxnState::Ongoing | TxnState::PrepareCommit | TxnState::PrepareAbort
            );
            let writes_to = |t: &str| e.rec.partitions.iter().any(|(p, _)| p == t);
            if !e.retired && busy && topic.is_none_or(writes_to) {
                return Some(e.rec.id.clone());
            }
        }
        None
    }
}
