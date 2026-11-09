//! Telling the open partitions that their topic's config, or the broker's defaults, have changed.

use std::sync::Arc;
use std::time::Duration;

use kafka_meta::Topic;
use kafka_storage::fs::Fs;
use kafka_wire::ids::TopicId;

use super::{Partition, Partitions};
use crate::topic_config;

impl<F: Fs> Partitions<F> {
    /// Hands the topics' present configs to their open partitions, which judge appends by them from then on. A
    /// partition not open reads its topic's config when it is opened. One being opened now is waited for (an open is a
    /// few file reads), since it may have read the config from before the change.
    pub(crate) async fn reconfigure(&self, topics: &[Arc<Topic>]) {
        let defaults = self.defaults.get();
        for topic in topics {
            let config = topic_config::log_config(&defaults, &topic.config);
            for partition in self.opening_or_open(topic.id).await {
                partition.configure(config).await;
            }
        }
    }

    async fn opening_or_open(&self, topic: TopicId) -> Vec<Arc<Partition<F>>> {
        let slots: Vec<_> = self
            .registry()
            .open
            .iter()
            .filter(|((id, _), _)| *id == topic)
            .map(|(_, e)| Arc::clone(&e.slot))
            .collect();
        let mut out = Vec::new();
        for slot in slots {
            // A slot is filled once its open has finished; one whose open failed is dropped by the registry, so
            // this stops waiting then too.
            for _ in 0..1000 {
                if slot.get().is_some() || Arc::strong_count(&slot) == 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            if let Some(p) = slot.get() {
                out.push(Arc::clone(p));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use kafka_storage::sim::SimFs;
    use kafka_wire::batch::build::BatchBuilder;
    use kafka_wire::batch::{Batch, TimestampType};
    use kafka_wire::ids::{PartitionId, TimestampMs};

    use super::*;
    use crate::partitions::Partition;
    use crate::partitions::registry::Slot;
    use crate::partitions::tests::setup_full;

    async fn write(p: &Partition<SimFs>) {
        let batch = Batch::parse(BatchBuilder::new().records(2).build()).unwrap();
        let pending = p
            .submit(
                batch,
                TimestampMs::from_wire(1),
                TimestampType::Create,
                true,
            )
            .await
            .unwrap();
        pending.await.unwrap().unwrap();
    }

    /// A partition that is being opened when its topic's config changes is waited for, and judges appends by the new
    /// config from then on (it may have read the old one before the change).
    #[tokio::test(start_paused = true)]
    async fn a_partition_still_opening_is_waited_for_and_given_the_new_config() {
        let (partitions, topic, ..) = setup_full(|_| {}, 1);
        let id = PartitionId::new(0).unwrap();
        let open = partitions.get(&topic, id).await.unwrap();
        // As it stands while its open is under way: its place is in the registry, and empty.
        let slot: Slot<SimFs> = Slot::default();
        let place = Arc::clone(&slot);
        partitions
            .registry()
            .open
            .get_mut(&(topic.id, id))
            .unwrap()
            .slot = place;
        let finish = async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            slot.set(Arc::clone(&open)).ok().unwrap();
        };
        let config = BTreeMap::from([("segment.bytes".to_owned(), "1".to_owned())]);
        let changed = Arc::new(Topic {
            config,
            ..(*topic).clone()
        });
        let topics = [changed];
        tokio::join!(partitions.reconfigure(&topics), finish);
        for _ in 0..4 {
            write(&open).await;
        }
        assert!(
            open.reader().segment_count() > 1,
            "a segment of one byte holds one batch: the new config was applied"
        );
    }
}
