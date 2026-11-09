//! The partitions that are open: each one a log with a task that owns its writer.
//!
//! A partition is opened the first time something needs it, and stays open until the least-recently-used limit
//! (`max_open_partitions`) closes it, flushed, to make room. Its writer is a task that takes
//! appends from a queue, writes everything that has queued, flushes once if any of it asked to be durable, and
//! so commits a group of producers with one flush. Readers do not go through it: a [`LogReader`] reads the
//! files directly.

mod actor;
mod registry;

use std::sync::{Arc, PoisonError, RwLock};

use kafka_storage::fs::Fs;
use kafka_storage::{LogReader, StorageError};
use kafka_wire::batch::{Batch, TimestampType};
use kafka_wire::ids::{Offset, TimestampMs};
use tokio::sync::{mpsc, oneshot};

use crate::txn_index::{Aborted, TxnIndex};

use actor::{Append, Command};
pub(crate) use actor::{TxnView, Written};
pub(crate) use registry::Partitions;

/// How many appends may wait for a partition's writer before the next sender waits.
pub(super) const QUEUE: usize = 32;

/// One open partition.
#[derive(Debug)]
pub(crate) struct Partition<F: Fs> {
    reader: LogReader<F>,
    commands: mpsc::Sender<Command>,
    /// The partition's transactions, kept by its writer and read here without asking it.
    txns: Arc<RwLock<TxnIndex>>,
}

/// What a `read_committed` consumer may be given of a partition: up to the last stable offset, skipping the aborted.
#[derive(Debug, Clone)]
pub(crate) struct Stability {
    /// The first offset a `read_committed` consumer may not read: where the oldest open transaction began, or the end.
    pub(crate) last_stable: Offset,
    /// The aborted transactions, whole, from the log start to the end.
    pub(crate) aborted: Vec<Aborted>,
}

/// The answer to an append that is on its way.
pub(crate) type Pending = oneshot::Receiver<Result<Written, StorageError>>;

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

    /// The last stable offset and the aborted transactions. Asked of the shared index, not of the writer: this does not
    /// wait for an append or a flush.
    pub(crate) fn stability(&self) -> Stability {
        // The end is read first: a transactional batch the log shows has been told to the index, which is locked while it
        // is written.
        let end = self.reader.end_offset();
        let index = self.txns.read().unwrap_or_else(PoisonError::into_inner);
        Stability {
            last_stable: Offset::new(index.last_stable(end.get())).unwrap_or(Offset::ZERO),
            aborted: index.aborted_between(self.reader.start_offset().get(), end.get()),
        }
    }

    /// The open transactions and the producers the writer remembers (read from the log the first time it is asked).
    ///
    /// # Errors
    /// The log cannot be read, or the writer has gone.
    pub(crate) async fn txns(&self) -> Result<TxnView, StorageError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(Command::Txns(reply))
            .await
            .map_err(|_| gone())?;
        answer.await.map_err(|_| gone())?
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
