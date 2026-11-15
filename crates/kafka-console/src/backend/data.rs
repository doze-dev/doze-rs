//! The broker's own data actions, which no Kafka request covers (docs/port/data-control.md): the usage of the data directory,
//! and Empty topic, Empty all topics and Erase everything. Each is asked as the console's principal, so the broker checks
//! the right it would ask a client for.

use kafka_broker::{DataRefusal, DataUsage, Emptied, EmptiedAll, Erased};

use super::Backend;

impl Backend {
    /// What is using the data directory, if the console may describe the cluster. The walk reads the disk, so it runs off the
    /// async threads.
    pub async fn data_usage(&self) -> Option<DataUsage> {
        let broker = std::sync::Arc::clone(&self.broker);
        let caller = self.caller.clone();
        tokio::task::spawn_blocking(move || broker.data_usage(&caller))
            .await
            .ok()
            .flatten()
    }

    /// Empties one topic.
    ///
    /// # Errors
    /// The broker's refusal.
    pub async fn empty_topic(&self, topic: &str) -> Result<Emptied, DataRefusal> {
        self.broker.empty_topic(&self.caller, topic).await
    }

    /// Empties every topic.
    ///
    /// # Errors
    /// The broker's refusal.
    pub async fn empty_topics(&self) -> Result<EmptiedAll, DataRefusal> {
        self.broker.empty_topics(&self.caller).await
    }

    /// Erases every topic, group and finished transactional id (the registry is the caller's to erase).
    ///
    /// # Errors
    /// The broker's refusal.
    pub async fn erase_everything(&self) -> Result<Erased, DataRefusal> {
        self.broker.erase_everything(&self.caller).await
    }
}
