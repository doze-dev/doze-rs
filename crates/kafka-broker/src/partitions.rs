//! The partitions that are open: each one a log with a task that owns its writer.
//!
//! A partition is opened the first time something needs it, and stays open. Its writer is a task that takes
//! appends from a queue, writes everything that has queued, flushes once if any of it asked to be durable, and
//! so commits a group of producers with one flush. Readers do not go through it: a [`LogReader`] reads the
//! files directly.

mod actor;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use kafka_meta::Topic;
use kafka_storage::fs::Fs;
use kafka_storage::topic_dirs::partition_dir;
use kafka_storage::{Appended, LogConfig, LogReader, PartitionLog, StorageError, Trust};
use kafka_wire::batch::{Batch, TimestampType};
use kafka_wire::ids::{PartitionId, TimestampMs, TopicId};
use tokio::sync::{OnceCell, mpsc, oneshot};

use crate::{Config, blocking, topic_config};
use actor::{Append, Command};

/// How many appends may wait for a partition's writer before the next sender waits.
const QUEUE: usize = 32;

/// One open partition.
#[derive(Debug)]
pub(crate) struct Partition<F: Fs> {
    reader: LogReader<F>,
    commands: mpsc::Sender<Command>,
}

/// The answer to an append that is on its way.
pub(crate) type Pending = oneshot::Receiver<Result<Appended, StorageError>>;

impl<F: Fs> Partition<F> {
    /// What can be read of the partition.
    pub(crate) fn reader(&self) -> &LogReader<F> {
        &self.reader
    }

    /// Queues a batch for the writer and returns where its answer will come. `durable` asks for the next flush
    /// to cover it (the answer's ticket then says when it has).
    ///
    /// # Errors
    /// The partition's writer has gone (the topic was deleted, or the broker is stopping).
    pub(crate) async fn submit(
        &self,
        batch: Batch,
        now: TimestampMs,
        clock: TimestampType,
        durable: bool,
    ) -> Result<Pending, StorageError> {
        let (reply, answer) = oneshot::channel();
        let append = Append {
            batch,
            now,
            clock,
            durable,
            reply,
        };
        self.commands
            .send(Command::Append(append))
            .await
            .map_err(|_| gone())?;
        Ok(answer)
    }

    /// Stops the writer after it has flushed what it has, and waits until it has.
    async fn close(&self) {
        let (done, closed) = oneshot::channel();
        if self.commands.send(Command::Close(done)).await.is_ok() {
            let _ = closed.await;
        }
    }
}

/// The error for a writer that is no longer there.
pub(crate) fn gone() -> StorageError {
    StorageError::Offline {
        cause: "the partition was closed".into(),
    }
}

type Slot<F> = Arc<OnceCell<Arc<Partition<F>>>>;

/// Every partition that has been opened, by topic id (so a topic deleted and made again is not the old one) and
/// number.
#[derive(Debug)]
pub(crate) struct Partitions<F: Fs> {
    fs: F,
    config: Config,
    open: Mutex<HashMap<(TopicId, PartitionId), Slot<F>>>,
}

impl<F: Fs> Partitions<F> {
    pub(crate) fn new(fs: F, config: Config) -> Self {
        Self {
            fs,
            config,
            open: Mutex::new(HashMap::new()),
        }
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
        let slot = {
            let mut open = self
                .open
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Arc::clone(open.entry((topic.id, partition)).or_default())
        };
        let opened = slot
            .get_or_try_init(|| async { self.open_one(topic, partition) })
            .await?;
        Ok(Arc::clone(opened))
    }

    fn open_one(
        &self,
        topic: &Topic,
        partition: PartitionId,
    ) -> Result<Arc<Partition<F>>, StorageError> {
        let dir = partition_dir(&self.config.data_dir, &topic.name, partition);
        let log_config: LogConfig = topic_config::log_config(&self.config.defaults, &topic.config);
        // Nothing vouches for the files yet (that is the clean-stop marker's job, when the broker records one), so
        // the first flush covers whatever is found.
        let log = blocking::run(|| {
            PartitionLog::open(self.fs.clone(), dir, log_config, Trust::Unvouched)
        })?;
        let reader = log.reader();
        let (commands, queue) = mpsc::channel(QUEUE);
        tokio::spawn(actor::run(log, queue, self.config.fsync_interval));
        Ok(Arc::new(Partition { reader, commands }))
    }

    /// Closes every open partition of `topic`, each after it has flushed, so that its files can be removed.
    pub(crate) async fn close_topic(&self, topic: TopicId) {
        let slots: Vec<Slot<F>> = {
            let mut open = self
                .open
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let keys: Vec<_> = open.keys().filter(|(t, _)| *t == topic).copied().collect();
            keys.iter().filter_map(|k| open.remove(k)).collect()
        };
        for slot in slots {
            if let Some(partition) = slot.get() {
                partition.close().await;
            }
        }
    }
}

#[cfg(test)]
impl<F: Fs> Partitions<F> {
    fn len(&self) -> usize {
        self.open.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests;
