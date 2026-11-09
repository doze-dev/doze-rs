//! The topics, in memory.

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use kafka_wire::ids::{TopicId, TopicName};
use redb::{Database, ReadableDatabase, ReadableTable};

use super::TOPICS;
use crate::names::NameIndex;
use crate::{MetaError, Topic};

#[derive(Debug, Default)]
pub(super) struct Registry {
    pub(super) by_name: HashMap<TopicName, Arc<Topic>>,
    pub(super) by_id: HashMap<TopicId, Arc<Topic>>,
    pub(super) names: NameIndex,
    /// Counts the changes, so that what is built from the topics can tell when to be built again.
    pub(super) version: u64,
}

impl Registry {
    pub(super) fn insert(&mut self, topic: Arc<Topic>) {
        self.version += 1;
        self.names.add(&topic.name);
        self.by_id.insert(topic.id, Arc::clone(&topic));
        self.by_name.insert(topic.name.clone(), topic);
    }

    /// A topic changed (its configuration, its partitions): the same name and id, a new record.
    pub(super) fn replace(&mut self, topic: Arc<Topic>) {
        self.version += 1;
        self.by_id.insert(topic.id, Arc::clone(&topic));
        self.by_name.insert(topic.name.clone(), topic);
    }

    pub(super) fn remove(&mut self, name: &TopicName) -> Option<Arc<Topic>> {
        let topic = self.by_name.remove(name)?;
        self.version += 1;
        self.by_id.remove(&topic.id);
        self.names.remove(name);
        Some(topic)
    }
}

/// The registry behind a lock held only for the length of a lookup or an insert.
#[derive(Debug, Default)]
pub(super) struct Shared(RwLock<Registry>);

impl Shared {
    /// Reads every topic out of the database.
    pub(super) fn load(db: &Database) -> Result<Self, MetaError> {
        let txn = db.begin_read()?;
        let table = txn.open_table(TOPICS)?;
        let mut registry = Registry::default();
        for entry in table.iter()? {
            let (name, record) = entry?;
            let topic = Topic::decode(name.value(), record.value())?;
            registry.insert(Arc::new(topic));
        }
        Ok(Self(RwLock::new(registry)))
    }

    pub(super) fn read(&self) -> RwLockReadGuard<'_, Registry> {
        self.0.read().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn write(&self) -> RwLockWriteGuard<'_, Registry> {
        self.0.write().unwrap_or_else(PoisonError::into_inner)
    }
}
