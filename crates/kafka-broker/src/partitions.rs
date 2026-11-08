//! The partitions that are open: each one a log with a task that owns its writer.
//!
//! A partition is opened the first time something needs it, and stays open. Its writer is a task that takes
//! appends from a queue, writes everything that has queued, flushes once if any of it asked to be durable, and
//! so commits a group of producers with one flush. Readers do not go through it: a [`LogReader`] reads the
//! files directly.

mod actor;
mod registry;

use kafka_storage::fs::Fs;
use kafka_storage::{Appended, LogReader, StorageError};
use kafka_wire::batch::{Batch, TimestampType};
use kafka_wire::ids::TimestampMs;
use tokio::sync::{mpsc, oneshot};

use actor::{Append, Command};
pub(crate) use registry::Partitions;

/// How many appends may wait for a partition's writer before the next sender waits.
pub(super) const QUEUE: usize = 32;

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

    /// Stops the writer after it has flushed what it has, and waits until it has. Says whether the flush worked.
    pub(super) async fn close(&self) -> bool {
        let (done, closed) = oneshot::channel();
        if self.commands.send(Command::Close(done)).await.is_err() {
            return false;
        }
        closed.await.unwrap_or(false)
    }
}

/// The error for a writer that is no longer there.
pub(crate) fn gone() -> StorageError {
    StorageError::Offline {
        cause: "the partition was closed".into(),
    }
}

#[cfg(test)]
mod tests;
