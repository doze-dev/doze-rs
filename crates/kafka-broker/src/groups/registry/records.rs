//! What the registry hands to a group's task: the topics there are, and where its records are kept.

use std::sync::Arc;

use kafka_meta::{ConsumerLife, GroupLife, MetaStore, ShareLife, StreamsLife};
use kafka_wire::ids::GroupId;

use crate::groups::actor::Records;
use crate::groups::consumer::{TopicInfo, TopicSource, Topics};
use crate::groups::group::Kept;

/// The topics there are, as the metadata store has them. The table a group is handed is built when the topics change, not
/// for each heartbeat.
#[derive(Debug)]
pub(super) struct MetaTopics(
    pub(super) Arc<MetaStore>,
    std::sync::Mutex<Option<(u64, Topics)>>,
);

impl MetaTopics {
    pub(super) fn new(meta: Arc<MetaStore>) -> Self {
        Self(meta, std::sync::Mutex::new(None))
    }
}

impl TopicSource for MetaTopics {
    fn topics(&self) -> Topics {
        let version = self.0.topics_version();
        let mut cached = self
            .1
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((at, topics)) = cached.as_ref()
            && *at == version
        {
            return topics.clone();
        }
        let topics = Topics::new(self.0.topics().iter().map(|t| TopicInfo {
            name: t.name.to_string(),
            id: t.id.to_uuid(),
            partitions: t.partitions.get(),
        }));
        *cached = Some((version, topics.clone()));
        topics
    }
}

/// A group's records, in the metadata database.
pub(super) struct StoreRecords {
    pub(super) meta: Arc<MetaStore>,
    pub(super) group: GroupId,
}

impl StoreRecords {
    fn write(&self, life: &GroupLife) -> bool {
        match crate::blocking::run(|| self.meta.set_group_life(&self.group, life)) {
            Ok(()) => true,
            Err(e) => {
                tracing::error!(group = %self.group, error = %e, "writing what is known of a group's members failed");
                false
            }
        }
    }
}

impl Records for StoreRecords {
    fn live(&self, session_ms: u32, protocol_type: &str, kept: &Kept) -> bool {
        let (consumer, share, streams) = lives(kept);
        self.write(&GroupLife {
            live: true,
            session_ms,
            empty_since_ms: None,
            protocol_type: protocol_type.to_owned(),
            consumer,
            share,
            streams,
        })
    }

    fn empty(&self, protocol_type: &str, kept: &Kept, now_ms: i64) -> bool {
        let (consumer, share, streams) = lives(kept);
        self.write(&GroupLife {
            live: false,
            session_ms: 0,
            // A consumer group's offsets are counted from each commit, so it has no time of becoming empty (and a share
            // group has none).
            empty_since_ms: matches!(kept, Kept::Classic).then_some(now_ms),
            protocol_type: protocol_type.to_owned(),
            consumer,
            share,
            streams,
        })
    }
}

type Lives = (Option<ConsumerLife>, Option<ShareLife>, Option<StreamsLife>);

fn lives(kept: &Kept) -> Lives {
    match kept {
        Kept::Classic => (None, None, None),
        Kept::Consumer(c) => (Some(c.clone()), None, None),
        Kept::Share(s) => (None, Some(s.clone()), None),
        Kept::Streams(s) => (None, None, Some(s.clone())),
    }
}
