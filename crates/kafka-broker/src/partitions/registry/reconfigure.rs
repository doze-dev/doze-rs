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
