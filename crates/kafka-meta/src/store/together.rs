//! Writes that arrive together share one transaction and one flush (group commit).
//!
//! An offset commit is a few bytes and its flush is what it costs: before this, every commit paid a flush of its own, one
//! at a time behind the lock topic changes take. Now a commit joins a queue, and the first caller to find nobody writing
//! becomes the writer: it takes that lock, then takes everything queued by then, judges and applies each write in the
//! order it arrived in one transaction, and flushes once. Every caller still answers only after its write is flushed,
//! and a flush that fails fails every write it held. Nobody waits for company: a caller alone is written at once, and
//! only callers that arrive while a flush is under way share the next one. No thread is started for it (the writer is
//! always one of the callers), so an idle broker holds nothing here.
//!
//! The same idea as `doze_store::GroupCommit`, which aws-sqs and aws-dynamodb use, without its writer thread: a write
//! here is judged against the topics under the lock topic changes take, which a thread of its own would have to share.
//!
//! A write that cannot be applied (a record that cannot be encoded, a read that failed) fails alone and before it has
//! written anything; the others in its transaction go on. A write that has nothing to store (every offset held, every
//! topic gone) is answered without a flush, and a batch with nothing to store opens no write transaction at all.

use std::collections::HashMap;
use std::io;
use std::sync::{Condvar, Mutex, PoisonError};

use kafka_wire::ids::{GroupId, PartitionId, TopicName};
use redb::WriteTransaction;

use super::{MetaStore, lock};
use crate::MetaError;
use crate::offsets::{Committed, OffsetCommit};
use crate::share::ShareState;

/// The most writes one transaction takes: bounds a batch's latency and its transaction's memory.
const MOST: usize = 1024;

/// A write that may share a flush.
#[derive(Debug)]
pub(super) enum Write {
    /// [`MetaStore::commit_offsets_unless_held`].
    Offsets {
        group: GroupId,
        commits: Vec<OffsetCommit>,
        within_ms: i64,
    },
    /// [`MetaStore::commit_zk_offsets`].
    ZkOffsets {
        group: GroupId,
        commits: Vec<(TopicName, PartitionId, i64)>,
    },
    /// [`MetaStore::set_share_states`].
    ShareStates {
        group: GroupId,
        states: Vec<(TopicName, PartitionId, ShareState)>,
    },
}

/// How one write of a batch went: `Ok(Ok(..))` applied (whether it stored anything, and each part's outcome),
/// `Ok(Err(..))` refused alone with nothing written, `Err(..)` a failure of the transaction, which fails the batch.
pub(super) type Applied = Result<Result<Vec<Committed>, MetaError>, MetaError>;

/// The transaction of a batch, begun only once a write has something to store.
pub(super) struct Txn<'a> {
    store: &'a MetaStore,
    txn: Option<WriteTransaction>,
}

impl Txn<'_> {
    /// The write transaction, begun now if no write of the batch has stored anything yet.
    ///
    /// # Errors
    /// The transaction could not be begun.
    pub(super) fn begun(&mut self) -> Result<&WriteTransaction, MetaError> {
        if self.txn.is_none() {
            self.txn = Some(self.store.begin()?);
        }
        self.txn
            .as_ref()
            .ok_or_else(|| MetaError::Corrupt("no write transaction".into()))
    }

    /// The write transaction if one was begun: reads go through it, so a write sees what earlier writes of its batch
    /// stored. With none begun, nothing has been written since the last flush, and a read of the database sees the same.
    pub(super) fn open(&self) -> Option<&WriteTransaction> {
        self.txn.as_ref()
    }
}

/// The queue, and whether one of its callers is writing.
#[derive(Debug, Default)]
pub(super) struct Together {
    queue: Mutex<Queue>,
    turn: Condvar,
}

#[derive(Debug, Default)]
struct Queue {
    next: u64,
    waiting: Vec<(u64, Write)>,
    answers: HashMap<u64, Result<Vec<Committed>, MetaError>>,
    writing: bool,
}

impl Together {
    /// How many writes are queued and not yet taken by a writer (a test waits on it).
    #[cfg(test)]
    pub(super) fn queued(&self) -> usize {
        lock(&self.queue).waiting.len()
    }
}

/// The writer's turn: whatever happens to the writer, the turn ends, its batch's callers are answered (with an error if
/// the writer never got to them), and the waiting are woken.
struct Turn<'a> {
    together: &'a Together,
    taken: Vec<u64>,
    answers: Vec<(u64, Result<Vec<Committed>, MetaError>)>,
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        let mut q = lock(&self.together.queue);
        q.writing = false;
        let mut answered: Vec<u64> = Vec::with_capacity(self.answers.len());
        for (ticket, answer) in self.answers.drain(..) {
            answered.push(ticket);
            q.answers.insert(ticket, answer);
        }
        for ticket in self.taken.drain(..) {
            if !answered.contains(&ticket) {
                let lost = io::Error::other("the metadata write was abandoned");
                q.answers.insert(ticket, Err(MetaError::Io(lost)));
            }
        }
        self.together.turn.notify_all();
    }
}

impl MetaStore {
    /// Applies `write` with whatever else is queued, in one transaction and one flush, and answers once it is durable.
    pub(super) fn together(&self, write: Write) -> Result<Vec<Committed>, MetaError> {
        let t = &self.together;
        let mut q = lock(&t.queue);
        let ticket = q.next;
        q.next = q.next.wrapping_add(1);
        q.waiting.push((ticket, write));
        loop {
            if let Some(answer) = q.answers.remove(&ticket) {
                return answer;
            }
            if q.writing {
                q = t.turn.wait(q).unwrap_or_else(PoisonError::into_inner);
                continue;
            }
            q.writing = true;
            drop(q);
            let mut turn = Turn {
                together: t,
                taken: Vec::new(),
                answers: Vec::new(),
            };
            self.write_batch(&mut turn);
            drop(turn);
            q = lock(&t.queue);
        }
    }

    /// One batch: what was queued by the time the lock topic changes take was had, applied in order, flushed once.
    fn write_batch(&self, turn: &mut Turn<'_>) {
        // Every write is judged against topics that cannot change until it is flushed.
        let _one_at_a_time = lock(&self.write);
        // Taken after the lock: whoever queued while it was waited for joins this flush.
        let batch: Vec<(u64, Write)> = {
            let mut q = lock(&self.together.queue);
            let n = q.waiting.len().min(MOST);
            q.waiting.drain(..n).collect()
        };
        turn.taken = batch.iter().map(|(ticket, _)| *ticket).collect();
        let mut txn = Txn {
            store: self,
            txn: None,
        };
        let mut answers = Vec::with_capacity(batch.len());
        for (ticket, write) in &batch {
            let applied = match write {
                Write::Offsets {
                    group,
                    commits,
                    within_ms,
                } => self.apply_offsets(&mut txn, group, commits, *within_ms),
                Write::ZkOffsets { group, commits } => {
                    self.apply_zk_offsets(&mut txn, group, commits)
                }
                Write::ShareStates { group, states } => {
                    self.apply_share_states(&mut txn, group, states)
                }
            };
            match applied {
                Ok(answer) => answers.push((*ticket, answer)),
                Err(e) => {
                    // The transaction is not to be trusted: nothing of the batch is stored, and every write in it that
                    // was not refused alone is told so, this one and those after it included.
                    let rest = batch
                        .iter()
                        .skip(answers.len())
                        .map(|(t, _)| (*t, Ok(Vec::new())));
                    answers.extend(rest);
                    turn.answers = failed(answers, e);
                    return;
                }
            }
        }
        // A batch that stored nothing has no transaction to flush.
        if let Some(txn) = txn.txn
            && let Err(e) = txn.commit()
        {
            turn.answers = failed(answers, e.into());
            return;
        }
        turn.answers = answers;
    }
}

/// The answers of a batch whose transaction failed: the first write that would have been stored gets the failure
/// itself, the others the same failure again; a write that had already been refused alone keeps its own error.
fn failed(
    answers: Vec<(u64, Result<Vec<Committed>, MetaError>)>,
    e: MetaError,
) -> Vec<(u64, Result<Vec<Committed>, MetaError>)> {
    let copy = again(&e);
    let mut first = Some(e);
    answers
        .into_iter()
        .map(|(ticket, answer)| match answer {
            Err(own) => (ticket, Err(own)),
            Ok(_) => (ticket, Err(first.take().unwrap_or_else(|| again(&copy)))),
        })
        .collect()
}

/// The same failure again, for another caller: a database or file failure as a file failure (both are answered alike),
/// an unreadable record as one.
fn again(e: &MetaError) -> MetaError {
    match e {
        MetaError::Corrupt(why) => MetaError::Corrupt(why.clone()),
        MetaError::Io(io) => MetaError::Io(io::Error::new(io.kind(), io.to_string())),
        other => MetaError::Io(io::Error::other(other.to_string())),
    }
}

#[cfg(test)]
mod tests;
