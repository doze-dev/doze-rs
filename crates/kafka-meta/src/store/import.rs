//! Taking over state another broker kept: topics with the identity they already have, and the producer ids it handed
//! out. Used once, by the importer of a Go doze-kafka data directory, before the broker serves anything.

use std::sync::Arc;

use redb::ReadableTable;

use super::{COUNTERS, MetaStore, TOPICS, lock};
use crate::{MetaError, Topic};

/// The counter [`MetaStore::next_producer_id`] reserves from.
const PRODUCER_IDS: &str = "producer_ids";

impl MetaStore {
    /// Stores `topic` as it is, its id included (a client that knows a topic by id keeps finding it). A topic of that
    /// name already here is left alone and `false` is returned.
    ///
    /// # Errors
    /// [`MetaError::Collides`] when the name collides with another under the `.` and `_` rule, or a failed write.
    pub fn import_topic(&self, topic: Topic) -> Result<bool, MetaError> {
        let _one_at_a_time = lock(&self.write);
        {
            let registry = self.shared.read();
            if registry.by_name.contains_key(&topic.name) {
                return Ok(false);
            }
            if let Some(collision) = registry
                .names
                .collision(&topic.name, &std::collections::HashSet::new())
            {
                return Err(MetaError::Collides(collision));
            }
        }
        let topic = Arc::new(topic);
        let record = topic.encode()?;
        let txn = self.begin()?;
        {
            txn.open_table(TOPICS)?
                .insert(topic.name.as_str(), record.as_slice())?;
        }
        txn.commit()?;
        self.shared.write().insert(topic);
        Ok(true)
    }

    /// Makes sure no producer id below `floor` is handed out again (ids already in the logs must stay unique).
    ///
    /// # Errors
    /// A failed write.
    pub fn raise_producer_ids(&self, floor: i64) -> Result<(), MetaError> {
        let floor = u64::try_from(floor).unwrap_or(0);
        let _ids = lock(&self.ids);
        let txn = self.begin()?;
        {
            let mut counters = txn.open_table(COUNTERS)?;
            let now = counters.get(PRODUCER_IDS)?.map_or(0, |v| v.value());
            if now < floor {
                counters.insert(PRODUCER_IDS, floor)?;
            }
        }
        txn.commit()?;
        Ok(())
    }
}
