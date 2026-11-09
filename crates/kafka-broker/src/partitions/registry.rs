//! The registry of open partitions, and the limit on how many stay open.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use kafka_meta::{MetaStore, Topic};
use kafka_storage::fs::Fs;
use kafka_storage::marker::StopState;
use kafka_storage::topic_dirs::partition_dir;
use kafka_storage::{LogConfig, PartitionLog, StorageError, Trust};
use kafka_wire::ids::{PartitionId, TopicId};
use tokio::sync::{OnceCell, mpsc};

mod closing;
mod reconfigure;

use super::{Partition, QUEUE, actor};
use crate::txn_index::TxnIndex;
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

/// What `Partitions::find` found.
enum Found<'a, F: Fs> {
    /// Open, and held.
    Open(Arc<Partition<F>>),
    /// Being closed: wait for that to be over.
    Closing(tokio::sync::futures::Notified<'a>),
    /// Not open yet; the slot to open it in.
    Unopened(Slot<F>),
}

#[derive(Debug)]
struct Registry<F: Fs> {
    open: HashMap<Key, Entry<F>>,
    clock: u64,
    /// What this run knows of a partition it has closed: flushed whole (so the next open can trust it) or not.
    closed: HashMap<String, Trust>,
    /// Partitions being closed now. Not opened again until the close is over, or two logs would be on the same files.
    closing: HashSet<Key>,
}

/// Every partition that has been opened, by topic id (so a topic deleted and made again is not the old one) and
/// number. At most `max_open` stay open: one over the limit closes the least recently used that nothing is using,
/// after flushing it, and it opens again when it is next asked for. That bounds the descriptors and memory a broker
/// with thousands of partitions needs to the ones in use.
#[derive(Debug)]
pub(crate) struct Partitions<F: Fs> {
    fs: F,
    config: Config,
    defaults: crate::Live,
    registry: Mutex<Registry<F>>,
    /// What the last stop vouched for.
    stop: StopState,
    /// Woken when a partition has finished closing.
    closing_done: tokio::sync::Notify,
    /// Which topics there are now: a partition is opened only for the topic that has its name at the time.
    meta: Arc<MetaStore>,
}

impl<F: Fs> Partitions<F> {
    pub(crate) fn new(
        fs: F,
        config: Config,
        defaults: crate::Live,
        stop: StopState,
        meta: Arc<MetaStore>,
    ) -> Self {
        Self {
            fs,
            config,
            defaults,
            meta,
            closing_done: tokio::sync::Notify::new(),
            registry: Mutex::new(Registry {
                open: HashMap::new(),
                clock: 0,
                closed: HashMap::new(),
                closing: HashSet::new(),
            }),
            stop,
        }
    }

    fn registry(&self) -> std::sync::MutexGuard<'_, Registry<F>> {
        self.registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// How many partitions are open now.
    pub(crate) fn open_count(&self) -> usize {
        self.registry()
            .open
            .values()
            .filter(|e| e.slot.get().is_some())
            .count()
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
        // A request that looked a topic up before it was deleted must not open it again: the directory is named for
        // the topic and may by now belong to a topic of the same name made since. Checked before the open, and after
        // it, since the delete can come between (it changes the metadata first and closes the partitions after).
        if !self.is_current(topic) {
            return Err(super::gone());
        }
        loop {
            let slot = match self.find(topic, partition) {
                Found::Open(open) => return Ok(open),
                Found::Closing(done) => {
                    done.await;
                    continue;
                }
                Found::Unopened(slot) => slot,
            };
            slot.get_or_try_init(|| async { self.open_one(topic, partition) })
                .await?;
            if !self.is_current(topic) {
                self.discard(key).await;
                return Err(super::gone());
            }
            self.evict(key).await;
        }
    }

    /// Where the partition stands, taken under the lock that eviction also takes, so that it cannot be closed between
    /// being found and being held. (No `await` here: the guard is not held across one.)
    fn find(&self, topic: &Topic, partition: PartitionId) -> Found<'_, F> {
        let key = (topic.id, partition);
        let mut registry = self.registry();
        if registry.closing.contains(&key) {
            // Made before the lock is let go, so that the wake-up cannot be missed.
            return Found::Closing(self.closing_done.notified());
        }
        registry.clock += 1;
        let tick = registry.clock;
        let entry = registry.open.entry(key).or_insert_with(|| Entry {
            name: format!("{}-{partition}", topic.name),
            slot: Slot::default(),
            used: 0,
        });
        entry.used = tick;
        match entry.slot.get() {
            Some(open) => Found::Open(Arc::clone(open)),
            None => Found::Unopened(Arc::clone(&entry.slot)),
        }
    }

    /// Whether `topic` is still the topic with its name.
    fn is_current(&self, topic: &Topic) -> bool {
        self.meta
            .topic(&topic.name)
            .is_some_and(|t| t.id == topic.id)
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
        let log_config: LogConfig = topic_config::log_config(&self.defaults.get(), &topic.config);
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
        // A log that holds transactions is read once, now, for the open and aborted ones; one that does not is not read.
        let index = if log.holds_transactions() {
            blocking::run(|| actor::read_index(&reader))?
        } else {
            TxnIndex::default()
        };
        let txns = actor::shared(index);
        let (commands, queue) = mpsc::channel(QUEUE);
        tokio::spawn(actor::run(
            log,
            queue,
            self.config.fsync_interval,
            Arc::clone(&txns),
        ));
        Ok(Arc::new(Partition {
            reader,
            commands,
            txns,
        }))
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
