//! The registry of open partitions, and the limit on how many stay open.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use kafka_meta::Topic;
use kafka_storage::fs::Fs;
use kafka_storage::marker::StopState;
use kafka_storage::topic_dirs::partition_dir;
use kafka_storage::{LogConfig, PartitionLog, StorageError, Trust};
use kafka_wire::ids::{PartitionId, TopicId};
use tokio::sync::{OnceCell, mpsc};
use tokio::task::JoinSet;

use super::{Partition, QUEUE, actor};
use crate::{Config, blocking, topic_config};

type Slot<F> = Arc<OnceCell<Arc<Partition<F>>>>;
type Key = (TopicId, PartitionId);

/// An open partition's place in the registry.
#[derive(Debug)]
struct Entry<F: Fs> {
    /// The name of its directory.
    name: String,
    slot: Slot<F>,
    /// When it was last asked for: a count that goes up with every request, so that the least recent is the least.
    used: u64,
}

#[derive(Debug)]
struct Registry<F: Fs> {
    open: HashMap<Key, Entry<F>>,
    clock: u64,
    /// What this run knows of a partition it has closed: flushed whole (so the next open can trust it) or not.
    closed: HashMap<String, Trust>,
}

/// Every partition that has been opened, by topic id (so a topic deleted and made again is not the old one) and
/// number. At most `max_open` stay open: one over the limit closes the least recently used that nothing is using,
/// after flushing it, and it opens again when it is next asked for. That bounds the descriptors and memory a broker
/// with thousands of partitions needs to the ones in use.
#[derive(Debug)]
pub(crate) struct Partitions<F: Fs> {
    fs: F,
    config: Config,
    registry: Mutex<Registry<F>>,
    /// What the last stop vouched for.
    stop: StopState,
}

impl<F: Fs> Partitions<F> {
    pub(crate) fn new(fs: F, config: Config, stop: StopState) -> Self {
        Self {
            fs,
            config,
            registry: Mutex::new(Registry {
                open: HashMap::new(),
                clock: 0,
                closed: HashMap::new(),
            }),
            stop,
        }
    }

    fn registry(&self) -> std::sync::MutexGuard<'_, Registry<F>> {
        self.registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The partition, opened if it is not yet. Two callers that arrive together share one open.
    ///
    /// # Errors
    /// The files cannot be opened.
    pub(crate) async fn get(
        &self,
        topic: &Topic,
        partition: PartitionId,
    ) -> Result<Arc<Partition<F>>, StorageError> {
        let key = (topic.id, partition);
        loop {
            let slot = {
                let mut registry = self.registry();
                registry.clock += 1;
                let tick = registry.clock;
                let entry = registry.open.entry(key).or_insert_with(|| Entry {
                    name: format!("{}-{partition}", topic.name),
                    slot: Slot::default(),
                    used: 0,
                });
                entry.used = tick;
                // Taken under the lock that eviction also takes, so a partition cannot be closed between being found
                // and being held.
                if let Some(open) = entry.slot.get() {
                    return Ok(Arc::clone(open));
                }
                Arc::clone(&entry.slot)
            };
            slot.get_or_try_init(|| async { self.open_one(topic, partition) })
                .await?;
            self.evict(key).await;
        }
    }

    /// The partition, unless it has never been written to: a partition that is not open and whose directory is empty
    /// is not opened (opening makes its first segment, and so a flush), because a listing of the offsets of
    /// thousands of new partitions would otherwise open each one.
    ///
    /// # Errors
    /// The directory cannot be listed, or the files cannot be opened.
    pub(crate) async fn get_unless_empty(
        &self,
        topic: &Topic,
        partition: PartitionId,
    ) -> Result<Option<Arc<Partition<F>>>, StorageError> {
        let open = self
            .registry()
            .open
            .get(&(topic.id, partition))
            .is_some_and(|e| e.slot.get().is_some());
        if !open {
            let dir = partition_dir(&self.config.data_dir, &topic.name, partition);
            let empty = blocking::run(|| self.fs.list_dir(&dir)).map(|entries| entries.is_empty());
            if empty
                .or_else(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        Ok(true)
                    } else {
                        Err(e)
                    }
                })
                .map_err(|source| StorageError::Io {
                    doing: "listing a partition directory",
                    source,
                })?
            {
                return Ok(None);
            }
        }
        self.get(topic, partition).await.map(Some)
    }

    fn open_one(
        &self,
        topic: &Topic,
        partition: PartitionId,
    ) -> Result<Arc<Partition<F>>, StorageError> {
        let dir = partition_dir(&self.config.data_dir, &topic.name, partition);
        let log_config: LogConfig = topic_config::log_config(&self.config.defaults, &topic.config);
        // Closed by this run: flushed whole, or not. Otherwise the last stop vouches for the files, or does not.
        let name = format!("{}-{partition}", topic.name);
        let trust = self
            .registry()
            .closed
            .get(&name)
            .copied()
            .unwrap_or_else(|| self.stop.trust(&name));
        let log = blocking::run(|| PartitionLog::open(self.fs.clone(), dir, log_config, trust))?;
        let reader = log.reader();
        let (commands, queue) = mpsc::channel(QUEUE);
        tokio::spawn(actor::run(log, queue, self.config.fsync_interval));
        Ok(Arc::new(Partition { reader, commands }))
    }

    /// Closes the partitions over the limit, least recently used first, except `keep` and any that something holds.
    async fn evict(&self, keep: Key) {
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

#[cfg(test)]
impl<F: Fs> Partitions<F> {
    pub(super) fn len(&self) -> usize {
        self.registry()
            .open
            .values()
            .filter(|e| e.slot.get().is_some())
            .count()
    }
}
