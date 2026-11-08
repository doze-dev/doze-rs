//! A topic's life, on disk and in the registry together.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use kafka_meta::Topic;
use kafka_storage::fs::Fs;
use kafka_storage::topic_dirs;
use kafka_wire::ids::{PartitionCount, TopicName};
use tracing::warn;

use crate::{Broker, BrokerError};

impl<F: Fs> Broker<F> {
    /// Registers a topic and makes its partition directories, so that it is there after a restart even if
    /// nothing is ever produced to it. The two are separate steps with no shared transaction, so a failure of
    /// the second takes the first back: otherwise the name stays registered, and every later request for it is
    /// told it exists and cannot be used.
    ///
    /// # Errors
    /// The topic exists or collides with one (nothing is written), or a write failed (nothing is left).
    pub(crate) fn make_topic(
        &self,
        name: &TopicName,
        partitions: PartitionCount,
        config: BTreeMap<String, String>,
        beside: &HashSet<TopicName>,
    ) -> Result<Arc<Topic>, BrokerError> {
        let topic = self.meta.create_topic(name, partitions, config, beside)?;
        let data = &self.config.data_dir;
        // The registry has just said the name is free, so a partition directory already on disk for it belongs
        // to no topic (a lost metadata database leaves them): adopting one would hand the new topic old records.
        match topic_dirs::remove_stray_dirs(&self.fs, data, name) {
            Ok(0) => {}
            Ok(n) => {
                warn!(topic = %name, directories = n, "removed leftover partition directories that belonged to no topic")
            }
            Err(e) => {
                warn!(topic = %name, error = %e, "could not clear leftover partition directories")
            }
        }
        if let Err(e) = topic_dirs::create_partition_dirs(&self.fs, data, name, partitions) {
            if let Err(undo) = self.meta.delete_topic(name) {
                warn!(topic = %name, error = %undo, "taking back the registration failed");
            }
            if let Err(undo) = topic_dirs::remove_topic_dirs(&self.fs, data, name, partitions) {
                warn!(topic = %name, error = %undo, "taking back the partition directories failed");
            }
            return Err(e.into());
        }
        Ok(topic)
    }

    /// Removes a topic from the registry and then its data from disk. Returns the topic, or `None` if there was
    /// none. A failure to clear the disk is logged and not reported: the topic is gone, and what is left is below
    /// no one's reach (the next topic of the name clears it).
    ///
    /// # Errors
    /// The registry could not be written; the topic is then still there.
    pub(crate) fn remove_topic(&self, name: &TopicName) -> Result<Option<Arc<Topic>>, BrokerError> {
        let removed = self.meta.delete_topic(name)?;
        if let Some(topic) = &removed {
            let cleared = topic_dirs::remove_topic_dirs(
                &self.fs,
                &self.config.data_dir,
                name,
                topic.partitions,
            );
            if let Err(e) = cleared {
                warn!(topic = %name, error = %e, "removing a deleted topic's partitions failed");
            }
        }
        Ok(removed)
    }
}
