//! Creating, finding and deleting topics.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use kafka_wire::ids::{PartitionCount, TopicId, TopicName};

use super::{MetaStore, TOPICS, lock};
use crate::{Collision, MetaError, Topic};

impl MetaStore {
    /// Creates a topic and makes it durable.
    ///
    /// `beside` is the topics the same request has already created: they do not count against the `.` and
    /// `_` rule (see [`MetaStore::collision`]).
    ///
    /// # Errors
    /// [`MetaError::Collides`] or [`MetaError::TopicExists`] (nothing is written), or a failed write.
    pub fn create_topic(
        &self,
        name: &TopicName,
        partitions: PartitionCount,
        config: BTreeMap<String, String>,
        beside: &HashSet<TopicName>,
    ) -> Result<Arc<Topic>, MetaError> {
        let _one_at_a_time = lock(&self.write);
        {
            let registry = self.shared.read();
            if let Some(collision) = registry.names.collision(name, beside) {
                return Err(MetaError::Collides(collision));
            }
            if registry.by_name.contains_key(name) {
                return Err(MetaError::TopicExists(name.clone()));
            }
        }
        let topic = Arc::new(Topic {
            name: name.clone(),
            id: random_topic_id()?,
            partitions,
            config,
        });
        let record = topic.encode()?;
        let txn = self.begin()?;
        {
            txn.open_table(TOPICS)?
                .insert(name.as_str(), record.as_slice())?;
        }
        txn.commit()?;
        self.shared.write().insert(Arc::clone(&topic));
        Ok(topic)
    }

    /// Deletes a topic and makes that durable. Returns it, or `None` if there was none.
    ///
    /// # Errors
    /// A failed write; the topic is then still there.
    pub fn delete_topic(&self, name: &TopicName) -> Result<Option<Arc<Topic>>, MetaError> {
        let _one_at_a_time = lock(&self.write);
        let Some(topic) = self.topic(name) else {
            return Ok(None);
        };
        let txn = self.begin()?;
        {
            txn.open_table(TOPICS)?.remove(name.as_str())?;
        }
        txn.commit()?;
        self.shared.write().remove(name);
        Ok(Some(topic))
    }

    /// The topic called `name`.
    #[must_use]
    pub fn topic(&self, name: &TopicName) -> Option<Arc<Topic>> {
        self.shared.read().by_name.get(name).cloned()
    }

    /// The topic with this id.
    #[must_use]
    pub fn topic_by_id(&self, id: TopicId) -> Option<Arc<Topic>> {
        self.shared.read().by_id.get(&id).cloned()
    }

    /// Every topic, by name.
    #[must_use]
    pub fn topics(&self) -> Vec<Arc<Topic>> {
        let mut all: Vec<_> = self.shared.read().by_name.values().cloned().collect();
        all.sort_by(|a, b| a.name.cmp(&b.name));
        all
    }

    /// The topic that `name` could not live beside, if there is one: Kafka's rule that `a.b` and `a_b` share
    /// metric names, and ours that names differing only in case share a directory. `beside` is the topics
    /// the same request has created, which the first rule lets off (Kafka checks each name of a request
    /// against the topics that existed before it).
    #[must_use]
    pub fn collision(&self, name: &TopicName, beside: &HashSet<TopicName>) -> Option<Collision> {
        self.shared.read().names.collision(name, beside)
    }
}

/// A new, random topic id (for a topic being made, and for the one a validate-only request is told).
///
/// # Errors
/// The system has no randomness to give.
pub fn random_topic_id() -> Result<TopicId, MetaError> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(TopicId::from_bytes(bytes))
}
