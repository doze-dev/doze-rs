//! The logs' own upkeep: segments that retention no longer keeps are deleted, now and then, in every topic.

use std::sync::Arc;
use std::time::Duration;

use kafka_storage::fs::Fs;
use kafka_wire::ids::PartitionId;

use crate::partitions::Trim;
use crate::{Broker, clock, topic_config};

impl<F: Fs> Broker<F> {
    /// Applies every topic's retention to every partition once, now. A partition that is not open and has never been
    /// written to is left as it is; one that has files is opened for it (and closed again when the limit on open
    /// partitions says), so that the writer, which owns the files of an open partition, does the deleting.
    pub async fn check_logs(&self) {
        let now = clock::now();
        for topic in self.meta.topics() {
            let policy = topic_config::retention(&self.defaults.get(), &topic.config);
            if !policy.is_set() {
                continue;
            }
            for n in 0..topic.partitions.get() {
                let Some(id) = PartitionId::new(n) else {
                    continue;
                };
                let swept = match self.partitions.get_unless_empty(&topic, id).await {
                    Ok(Some(partition)) => {
                        partition.trim(Trim::Retention(policy, now)).await.map(drop)
                    }
                    Ok(None) => Ok(()),
                    Err(e) => Err(e),
                };
                if let Err(error) = swept {
                    tracing::warn!(topic = %topic.name, partition = n, %error, "applying retention to a partition failed");
                }
            }
        }
    }

    /// Applies retention now and then, as long as the broker lives.
    pub fn start_log_sweeper(self: &Arc<Self>) -> tokio::task::JoinHandle<()>
    where
        F: Send + Sync + 'static,
    {
        let broker = Arc::downgrade(self);
        let every: Duration = self.config.log_retention_check_every;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(every).await;
                let Some(broker) = broker.upgrade() else {
                    break;
                };
                broker.check_logs().await;
            }
        })
    }
}
