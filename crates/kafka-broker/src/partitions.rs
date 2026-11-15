//! The partitions that are open: each one a log with a task that owns its writer.
//!
//! A partition is opened the first time something needs it, and stays open until the least-recently-used limit
//! (`max_open_partitions`) closes it, flushed, to make room, or until nothing has asked for it for
//! `partition_idle_close` (five minutes). Its writer is a task that takes
//! appends from a queue, writes everything that has queued, flushes once if any of it asked to be durable, and
//! so commits a group of producers with one flush. Readers do not go through it: a [`LogReader`] reads the
//! files directly.

mod actor;
mod registry;
mod upkeep;

use std::sync::{Arc, PoisonError, RwLock};

use kafka_storage::fs::Fs;
use kafka_storage::{LogReader, StorageError};
use kafka_wire::batch::{Batch, TimestampType};
use kafka_wire::ids::{Offset, TimestampMs};
use tokio::sync::{mpsc, oneshot};

use crate::producers::FirstSequence;
use crate::txn_index::{Aborted, TxnIndex};

use actor::{Append, Command};

/// How many of a round's segments the writer puts in place in one go: each takes three directory flushes (about 13 ms
/// measured under load), so this keeps an append from waiting more than a fifth of a second for a round.
const INSTALL_AT_A_TIME: usize = 16;
pub(crate) use actor::{Retained, Trim, TxnView, Written};
pub(crate) use registry::Partitions;
pub(crate) use upkeep::{Chore, Upkeep, Verdict, settings_print};

/// How many appends may wait for a partition's writer before the next sender waits.
pub(super) const QUEUE: usize = 32;

/// One open partition.
#[derive(Debug)]
pub(crate) struct Partition<F: Fs> {
    reader: LogReader<F>,
    commands: mpsc::Sender<Command<F>>,
    /// The partition's transactions, kept by its writer and read here without asking it.
    txns: Arc<RwLock<TxnIndex>>,
}

/// A partition's transactions as the console shows them: the last stable offset and every aborted transaction.
#[derive(Debug, Clone)]
pub(crate) struct Stability {
    /// The first offset a `read_committed` consumer may not read: where the oldest open transaction began, or the end.
    pub(crate) last_stable: Offset,
    /// The aborted transactions, whole, from the log start to the end.
    #[cfg_attr(not(feature = "inspect"), allow(dead_code))]
    pub(crate) aborted: Vec<Aborted>,
}

/// The log as a look at its transaction index saw it.
#[derive(Clone, Copy)]
struct Seen {
    start: i64,
    end: i64,
    /// The last stable offset.
    stable: Offset,
}

/// The answer to an append that is on its way.
pub(crate) type Pending = oneshot::Receiver<Result<Written, StorageError>>;

impl<F: Fs> Partition<F> {
    /// What can be read of the partition.
    pub(crate) fn reader(&self) -> &LogReader<F> {
        &self.reader
    }

    /// The files this partition holds open: two for each segment, one for flushing its directory and one of headroom.
    pub(crate) fn descriptors(&self) -> usize {
        self.reader.segment_count() * 2 + 2
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
        self.submit_with(batch, now, clock, durable, FirstSequence::Any)
            .await
    }

    /// [`Self::submit`], saying where a producer the partition knows no batch of may start its sequences.
    ///
    /// # Errors
    /// The partition's writer has gone.
    pub(crate) async fn submit_with(
        &self,
        batch: Batch,
        now: TimestampMs,
        clock: TimestampType,
        durable: bool,
        first: FirstSequence,
    ) -> Result<Pending, StorageError> {
        let (reply, answer) = oneshot::channel();
        let append = Append {
            batch,
            now,
            clock,
            durable,
            first,
            reply,
        };
        self.commands
            .send(Command::Append(append))
            .await
            .map_err(|_| gone())?;
        Ok(answer)
    }

    /// The last stable offset. Asked of the shared index, not of the writer: this does not wait for an append or a flush.
    pub(crate) fn last_stable(&self) -> Offset {
        self.with_index(|_, seen| seen.stable)
    }

    /// What a `read_committed` reader from `from` may be given: the last stable offset, and each aborted transaction that
    /// touches `from` up to it, as `each` makes it. One look at the index, so the two agree.
    pub(crate) fn committed<T>(
        &self,
        from: i64,
        each: impl FnMut(&Aborted) -> T,
    ) -> (Offset, Vec<T>) {
        self.with_index(|index, seen| {
            let aborted = index
                .aborted_between(from.max(seen.start), seen.stable.get())
                .map(each)
                .collect();
            (seen.stable, aborted)
        })
    }

    /// The last stable offset and every aborted transaction the log holds (the console's view of its producers).
    pub(crate) fn stability(&self) -> Stability {
        self.with_index(|index, seen| Stability {
            last_stable: seen.stable,
            aborted: index
                .aborted_between(seen.start, seen.end)
                .copied()
                .collect(),
        })
    }

    /// `look` at the index, under its read lock, with what the log was seen to be.
    fn with_index<T>(&self, look: impl FnOnce(&TxnIndex, Seen) -> T) -> T {
        // The end is read first: a transactional batch the log shows has been told to the index, which is locked while it
        // is written.
        let end = self.reader.end_offset().get();
        let index = self.txns.read().unwrap_or_else(PoisonError::into_inner);
        // A log start moved past an open transaction's first offset (DeleteRecords) holds the last stable offset up to it.
        let start = self.reader.start_offset().get();
        let stable =
            Offset::new(clamp_stable(index.last_stable(end), start, end)).unwrap_or(Offset::ZERO);
        look(&index, Seen { start, end, stable })
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

    /// Each producer's window of recent batches, for the console.
    ///
    /// # Errors
    /// The log cannot be read, or the writer has gone.
    #[cfg_attr(not(feature = "inspect"), allow(dead_code))]
    pub(crate) async fn windows(&self) -> Result<Vec<crate::producers::Window>, StorageError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(Command::Windows(reply))
            .await
            .map_err(|_| gone())?;
        answer.await.map_err(|_| gone())?
    }

    /// Moves the start of the log up, through the writer, so that it happens between appends. The start now in force.
    ///
    /// # Errors
    /// The offset is past the end of the log, the partition is offline, a write failed, or the writer has gone.
    pub(crate) async fn trim(&self, how: Trim) -> Result<Offset, StorageError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(Command::Trim(how, reply))
            .await
            .map_err(|_| gone())?;
        answer.await.map_err(|_| gone())?
    }

    /// Applies retention, through the writer, and says when it could next take something.
    ///
    /// # Errors
    /// A removal or a read of a segment's age failed, or the writer has gone.
    pub(crate) async fn retain(
        &self,
        policy: kafka_storage::Retention,
        now: TimestampMs,
    ) -> Result<Retained, StorageError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(Command::Retain(policy, now, reply))
            .await
            .map_err(|_| gone())?;
        answer.await.map_err(|_| gone())?
    }

    /// A round of cleaning, if the log is due one, planned by the writer; with the writer's count of changes when it was
    /// judged.
    ///
    /// # Errors
    /// The log's history cannot be read, or the writer has gone.
    pub(crate) async fn plan_clean(
        &self,
        params: kafka_storage::compact::Params,
    ) -> Result<(kafka_storage::compact::Planned<F>, u64), StorageError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(Command::Clean(params, reply))
            .await
            .map_err(|_| gone())?;
        answer.await.map_err(|_| gone())?
    }

    /// Has the writer put a round's new segments in place; how many it replaced.
    ///
    /// # Errors
    /// A step failed, or the writer has gone.
    pub(crate) async fn install_clean(
        &self,
        cleaned: kafka_storage::compact::Cleaned,
    ) -> Result<usize, StorageError> {
        // A few segments at a time: between two parts the writer takes the appends and reads that have waited.
        let mut parts = cleaned.in_parts(INSTALL_AT_A_TIME).into_iter();
        let mut replaced = 0;
        while let Some(part) = parts.next() {
            let (reply, answer) = oneshot::channel();
            self.commands
                .send(Command::Install(part, reply))
                .await
                .map_err(|_| gone())?;
            match answer.await.map_err(|_| gone())? {
                Ok(n) => replaced += n,
                Err(e) => {
                    for rest in parts {
                        let _ = self.commands.send(Command::Discard(rest)).await;
                    }
                    return Err(e);
                }
            }
        }
        Ok(replaced)
    }

    /// Tells the writer to judge appends by `config` from now on, between two appends.
    pub(crate) async fn configure(&self, config: kafka_storage::LogConfig) {
        // A writer that has gone is a partition that is closing, and it will open with the config it is given then.
        let _ = self.commands.send(Command::Configure(config)).await;
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

/// The last stable offset kept inside the log: not below its start (DeleteRecords moved the start past an open
/// transaction), and never above the end read before it (a delete to the end and appends can come between the reads of
/// the end and the start).
fn clamp_stable(last_stable: i64, start: i64, end: i64) -> i64 {
    last_stable.max(start).min(end)
}

#[cfg(test)]
mod tests;
