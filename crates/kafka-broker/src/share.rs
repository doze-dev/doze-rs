//! Share groups' records (KIP-932, "Queues for Kafka"): the share-partitions, the share sessions, and the alarm of the
//! acquisition locks. The membership of a share group is a group like the others (`groups::share`); what is here is what
//! Kafka keeps on the partition leader (`SharePartitionManager`) and in the share coordinator (`__share_group_state`,
//! here the metadata database's `share_states`).
//!
//! Nothing here allocates, and no task wakes, until a share group fetches: the map of share-partitions is empty, the
//! session cache is empty, and the lock task waits on an alarm that nothing has set.

mod load;
mod locks;
pub(crate) mod partition;
pub(crate) mod sessions;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use kafka_wire::Uuid;
use kafka_wire::ids::{GroupId, PartitionId, TopicName};
use tokio::sync::watch;

use crate::alarm::Alarm;
pub(crate) use load::limits_of;
pub(crate) use partition::SharePartition;
pub(crate) use sessions::Sessions;

/// One share-partition, by group, topic id and partition.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    pub(crate) group: GroupId,
    pub(crate) topic: Uuid,
    pub(crate) partition: i32,
}

/// A share-partition in memory, with the topic name its state is stored under.
#[derive(Debug)]
pub(crate) struct Held {
    pub(crate) topic: TopicName,
    pub(crate) partition: PartitionId,
    part: Mutex<SharePartition>,
}

impl Held {
    pub(crate) fn new(topic: TopicName, partition: PartitionId, part: SharePartition) -> Self {
        Self {
            topic,
            partition,
            part: Mutex::new(part),
        }
    }

    /// The share-partition, locked: never held across an `.await`.
    pub(crate) fn lock(&self) -> MutexGuard<'_, SharePartition> {
        self.part.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The share-partitions in memory, the share sessions, and the lock alarm.
#[derive(Debug)]
pub(crate) struct Shares {
    parts: Mutex<HashMap<Key, Arc<Held>>>,
    sessions: Mutex<Sessions>,
    /// The soonest an acquisition lock runs out.
    pub(crate) alarm: Alarm,
    /// Moved on each change that may let a waiting share fetch acquire something: records made available again, a start
    /// that moved.
    changed: watch::Sender<u64>,
}

impl Shares {
    #[must_use]
    pub(crate) fn new(most_sessions: usize) -> Self {
        Self {
            parts: Mutex::new(HashMap::new()),
            sessions: Mutex::new(Sessions::new(most_sessions)),
            alarm: Alarm::unset(),
            changed: watch::channel(0).0,
        }
    }

    fn table(&self) -> MutexGuard<'_, HashMap<Key, Arc<Held>>> {
        self.parts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The session cache, locked: never held across an `.await`.
    pub(crate) fn sessions(&self) -> MutexGuard<'_, Sessions> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn get(&self, key: &Key) -> Option<Arc<Held>> {
        self.table().get(key).cloned()
    }

    /// Puts a share-partition in memory, unless another request did meanwhile (that one is kept).
    pub(crate) fn insert(&self, key: Key, held: Held) -> Arc<Held> {
        Arc::clone(self.table().entry(key).or_insert_with(|| Arc::new(held)))
    }

    /// Every share-partition in memory.
    pub(crate) fn all(&self) -> Vec<(Key, Arc<Held>)> {
        self.table()
            .iter()
            .map(|(k, h)| (k.clone(), Arc::clone(h)))
            .collect()
    }

    /// Lets go of a group's share-partitions (of the topics named, or all): they are read again from the store when next
    /// fetched.
    pub(crate) fn forget(&self, group: &GroupId, topics: Option<&[Uuid]>) {
        self.table()
            .retain(|k, _| k.group != *group || topics.is_some_and(|t| !t.contains(&k.topic)));
    }

    /// Lets go of every share-partition of a topic that is gone.
    pub(crate) fn forget_topic(&self, topic: Uuid) {
        self.table().retain(|k, _| k.topic != topic);
    }

    /// Something may be acquirable now that was not.
    pub(crate) fn notify(&self) {
        self.changed.send_modify(|n| *n = n.wrapping_add(1));
    }

    /// What a waiting share fetch watches.
    pub(crate) fn watch(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }
}
