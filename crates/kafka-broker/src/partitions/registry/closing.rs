//! Closing partitions: to keep within the limit, to remove a topic's files, and at a stop.

use std::collections::HashMap;
use std::sync::Arc;

use kafka_storage::Trust;
use kafka_storage::fs::Fs;
use kafka_wire::ids::TopicId;
use tokio::task::JoinSet;

use super::{Entry, Key, Partition, Partitions};

/// A partition's close, in progress: ends, and wakes whoever waits to open it again, however it is left.
struct Closing<'a, F: Fs> {
    partitions: &'a Partitions<F>,
    key: Key,
}

impl<F: Fs> Drop for Closing<'_, F> {
    fn drop(&mut self) {
        self.partitions.registry().closing.remove(&self.key);
        self.partitions.closing_done.notify_waiters();
    }
}

impl<F: Fs> Partitions<F> {
    /// Pretends the partition at `key` is being closed, until the returned value is dropped.
    #[cfg(test)]
    pub(in crate::partitions) fn pretend_closing(&self, key: Key) -> impl Drop + '_ {
        self.registry().closing.insert(key);
        Closing {
            partitions: self,
            key,
        }
    }

    /// Closes a partition that was opened for a topic that has gone, and forgets it.
    pub(super) async fn discard(&self, key: Key) {
        let entry = self.registry().open.remove(&key);
        if let Some(partition) = entry.as_ref().and_then(|e| e.slot.get()) {
            let _ = partition.close().await;
        }
    }

    /// Closes the partitions over the limits (how many are open, and how many files they hold open between them), least
    /// recently used first, except `keep` and any that something holds.
    pub(super) async fn evict(&self, keep: Key) {
        let (max, budget) = (
            self.config.max_open_partitions,
            self.config.max_open_descriptors,
        );
        if max == 0 && budget == 0 {
            return;
        }
        let victims = self.pick_victims(keep, (max, budget));
        // Every guard exists before the first close begins, so that dropping this future part way releases them all.
        let work: Vec<_> = victims
            .into_iter()
            .map(|(key, name, partition)| {
                (
                    Closing {
                        partitions: self,
                        key,
                    },
                    name,
                    partition,
                )
            })
            .collect();
        for (done, name, partition) in work {
            if partition.close().await {
                self.registry().closed.insert(name, Trust::Vouched);
            }
            drop(done);
        }
    }

    /// Takes out of the registry the partitions to close, least recently used first, until the limits `(most partitions, most
    /// files)` are met or only the busy ones are left; they are marked as closing.
    fn pick_victims(
        &self,
        keep: Key,
        (max, budget): (usize, usize),
    ) -> Vec<(Key, String, Arc<Partition<F>>)> {
        let mut registry = self.registry();
        let opened = || {
            registry
                .open
                .values()
                .filter_map(|e| e.slot.get())
                .map(|p| p.descriptors())
        };
        let (mut open, mut held) = (opened().count(), opened().sum::<usize>());
        let over =
            |open: usize, held: usize| (max != 0 && open > max) || (budget != 0 && held > budget);
        if !over(open, held) {
            return Vec::new();
        }
        let mut idle: Vec<_> = registry
            .open
            .iter()
            .filter(|(k, e)| **k != keep && e.slot.get().is_some_and(|p| Arc::strong_count(p) == 1))
            .map(|(k, e)| (e.used, *k))
            .collect();
        idle.sort_unstable();
        // The least recently used go until both limits are met (or nothing idle is left to close).
        let mut take: Vec<Key> = Vec::new();
        for (_, k) in idle {
            if !over(open, held) {
                break;
            }
            if let Some(p) = registry.open.get(&k).and_then(|e| e.slot.get()) {
                open -= 1;
                held = held.saturating_sub(p.descriptors());
                take.push(k);
            }
        }
        let mut victims = Vec::new();
        for k in take {
            if let Some(e) = registry.open.remove(&k)
                && let Some(p) = e.slot.get()
            {
                // Not vouched for until the close says so: if this future is dropped half way (a stop), the
                // partition's files are not known to be whole.
                registry.closed.insert(e.name.clone(), Trust::Unvouched);
                registry.closing.insert(k);
                victims.push((k, e.name, Arc::clone(p)));
            }
        }
        victims
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
