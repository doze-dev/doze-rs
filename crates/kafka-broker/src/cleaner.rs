//! Compaction, for the topics whose policy asks for it: now and then each partition is asked whether its log is due to be
//! cleaned (its writer says, knowing what the log holds), the round is run away from the writer on the segments' own
//! readers, and the writer puts what it made in place.

use std::sync::Arc;
use std::time::Duration;

use kafka_meta::Topic;
use kafka_storage::StorageError;
use kafka_storage::fs::Fs;
use kafka_wire::ids::PartitionId;

use crate::{Broker, clock, topic_config};

impl<F: Fs> Broker<F> {
    /// Cleans every compacted topic's partitions that are due, once, now. A partition that is not open and has never been
    /// written to is left as it is.
    pub async fn check_cleaner(&self) {
        self.log_notes.looked(clock::now().get());
        for topic in self.meta.topics() {
            if !topic_config::compacts(&topic.config) {
                continue;
            }
            for n in 0..topic.partitions.get() {
                let Some(id) = PartitionId::new(n) else {
                    continue;
                };
                if let Err(error) = self.clean_partition(&topic, id).await {
                    self.log_notes.failed(
                        topic.id,
                        id.get(),
                        clock::now().get(),
                        &error.to_string(),
                    );
                    if self
                        .stop_cleaning
                        .load(std::sync::atomic::Ordering::Relaxed)
                    {
                        return;
                    }
                    tracing::warn!(topic = %topic.name, partition = n, %error, "cleaning a partition failed");
                }
            }
        }
    }

    pub(crate) async fn clean_partition(
        &self,
        topic: &Topic,
        id: PartitionId,
    ) -> Result<(), StorageError> {
        let was_open = self.partitions.is_open(topic.id, id);
        let Some(partition) = self.partitions.get_unless_empty(topic, id).await? else {
            return Ok(());
        };
        let result = self.clean_open(topic, id, &partition).await;
        drop(partition);
        if !was_open {
            self.partitions.release_if_idle(topic.id, id).await;
        }
        result
    }

    async fn clean_open(
        &self,
        topic: &Topic,
        id: PartitionId,
        partition: &crate::partitions::Partition<F>,
    ) -> Result<(), StorageError> {
        let params = topic_config::cleaner(
            &self.defaults.get(),
            &topic.config,
            clock::now().get(),
            self.config.cleaner_map_bytes,
        );
        let Some(job) = partition.plan_clean(params).await? else {
            return Ok(());
        };
        let (began, clock_began) = (std::time::Instant::now(), clock::now().get());
        self.log_notes.running(topic.id, id.get(), clock_began);
        let stop = Arc::clone(&self.stop_cleaning);
        let cleaned = tokio::task::spawn_blocking(move || job.run_until(&stop))
            .await
            .map_err(|e| StorageError::Io {
                doing: "cleaning a log",
                source: std::io::Error::other(e.to_string()),
            })??;
        let (read, kept, clean_to) = (cleaned.records_read, cleaned.records_kept, cleaned.clean_to);
        let replaced = partition.install_clean(cleaned).await?;
        self.log_notes.round(
            topic.id,
            id.get(),
            crate::log_notes::RoundNote {
                at_ms: clock::now().get(),
                took_ms: u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX),
                read,
                kept,
                replaced,
                clean_to,
            },
        );
        if replaced > 0 {
            tracing::debug!(topic = %topic.name, partition = %id, segments = replaced, read, kept, "cleaned a log");
        }
        Ok(())
    }

    /// Cleans now and then, as long as the broker lives.
    pub fn start_log_cleaner(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let broker = Arc::downgrade(self);
        let every: Duration = self.config.log_cleaner_every;
        let enabled = self.config.log_cleaner_enable;
        tokio::spawn(async move {
            if !enabled {
                return;
            }
            loop {
                tokio::time::sleep(every).await;
                let Some(broker) = broker.upgrade() else {
                    break;
                };
                broker.check_cleaner().await;
            }
        })
    }
}
