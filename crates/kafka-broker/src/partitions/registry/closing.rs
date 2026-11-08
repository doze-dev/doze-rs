//! Closing partitions: to keep within the limit, to remove a topic's files, and at a stop.

use std::collections::HashMap;
use std::sync::Arc;

use kafka_storage::Trust;
use kafka_storage::fs::Fs;
use kafka_wire::ids::TopicId;
use tokio::task::JoinSet;

use super::{Entry, Key, Partition, Partitions};

impl<F: Fs> Partitions<F> {
    /// Closes a partition that was opened for a topic that has gone, and forgets it.
    pub(super) async fn discard(&self, key: Key) {
        let entry = self.registry().open.remove(&key);
        if let Some(partition) = entry.as_ref().and_then(|e| e.slot.get()) {
            let _ = partition.close().await;
        }
    }

    /// Closes the partitions over the limit, least recently used first, except `keep` and any that something holds.
    pub(super) async fn evict(&self, keep: Key) {
        let max = self.config.max_open_partitions;
        if max == 0 {
            return;
        }
        let victims: Vec<(String, Arc<Partition<F>>)> = {
            let mut registry = self.registry();
            let open = registry
                .open
                .values()
                .filter(|e| e.slot.get().is_some())
                .count();
            if open <= max {
                return;
            }
            let mut idle: Vec<_> = registry
                .open
                .iter()
                .filter(|(k, e)| {
                    **k != keep && e.slot.get().is_some_and(|p| Arc::strong_count(p) == 1)
                })
                .map(|(k, e)| (e.used, *k))
                .collect();
            idle.sort_unstable();
            let take: Vec<Key> = idle.into_iter().take(open - max).map(|(_, k)| k).collect();
            take.iter()
                .filter_map(|k| registry.open.remove(k))
                .filter_map(|e| Some((e.name, e.slot.get()?.clone())))
                .collect()
        };
        for (name, partition) in victims {
            let trust = if partition.close().await {
                Trust::Vouched
            } else {
                Trust::Unvouched
            };
            self.registry().closed.insert(name, trust);
        }
    }

    /// Closes every open partition of `topic`, each after it has flushed, so that its files can be removed.
    pub(crate) async fn close_topic(&self, topic: TopicId) {
        let entries: Vec<Entry<F>> = {
            let mut registry = self.registry();
            let keys: Vec<_> = registry
                .open
                .keys()
                .filter(|(t, _)| *t == topic)
                .copied()
                .collect();
            let entries: Vec<_> = keys
                .iter()
                .filter_map(|k| registry.open.remove(k))
                .collect();
            // The directories are about to go: what was known of them goes too.
            for e in &entries {
                registry.closed.remove(&e.name);
            }
            entries
        };
        for e in entries {
            if let Some(partition) = e.slot.get() {
                let _ = partition.close().await;
            }
        }
    }

    /// Closes every open partition after it has flushed, and says, for each partition this run opened, whether its
    /// last close left files the next start can trust. A partition this run never opened is not in the answer.
    pub(crate) async fn close_all(&self) -> HashMap<String, Trust> {
        let entries: Vec<Entry<F>> = self.registry().open.drain().map(|(_, e)| e).collect();
        let mut closing = JoinSet::new();
        for e in entries {
            if let Some(partition) = e.slot.get().cloned() {
                closing.spawn(async move {
                    let flushed = partition.close().await;
                    let trust = if flushed {
                        Trust::Vouched
                    } else {
                        Trust::Unvouched
                    };
                    (e.name, trust)
                });
            }
        }
        let mut verdicts: HashMap<String, Trust> = self.registry().closed.clone();
        while let Some(done) = closing.join_next().await {
            // A close that panicked is a partition nobody can vouch for; its name went with it, and the directories
            // the last stop did not vouch for stay unvouched.
            if let Ok((name, trust)) = done {
                verdicts.insert(name, trust);
            }
        }
        verdicts
    }
}
