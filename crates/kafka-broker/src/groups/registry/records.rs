//! What the registry hands to a group's task: the topics there are, and where its records are kept.

use std::sync::Arc;

use kafka_meta::{ConsumerLife, GroupLife, MetaStore};
use kafka_wire::ids::GroupId;

use crate::groups::actor::Records;
use crate::groups::consumer::{TopicInfo, TopicSource, Topics};

/// The topics there are, as the metadata store has them.
pub(super) struct MetaTopics(pub(super) Arc<MetaStore>);

impl TopicSource for MetaTopics {
    fn topics(&self) -> Topics {
        Topics::new(self.0.topics().iter().map(|t| TopicInfo {
            name: t.name.to_string(),
            id: t.id.to_uuid(),
            partitions: t.partitions.get(),
        }))
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
    fn live(&self, session_ms: u32, protocol_type: &str, consumer: Option<ConsumerLife>) -> bool {
        self.write(&GroupLife {
            live: true,
            session_ms,
            empty_since_ms: None,
            protocol_type: protocol_type.to_owned(),
            consumer,
        })
    }

    fn empty(&self, protocol_type: &str, consumer: Option<ConsumerLife>, now_ms: i64) -> bool {
        self.write(&GroupLife {
            live: false,
            session_ms: 0,
            // A consumer group's offsets are counted from each commit, so it has no time of becoming empty.
            empty_since_ms: consumer.is_none().then_some(now_ms),
            protocol_type: protocol_type.to_owned(),
            consumer,
        })
    }
}
