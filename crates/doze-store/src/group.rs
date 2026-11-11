//! Group commit: one writer thread owns the writes, takes whatever is queued, applies it all in one
//! transaction and flushes once.
//!
//! Every caller still gets a durable answer: its [`Ack`] resolves only after the commit that holds its
//! operation has been flushed. Under load many callers share a flush (spike S6 measured p50 about 6 ms
//! and p99 under 15 ms for durable sends at 10,000 a second on one core); alone, a caller pays one flush.
//!
//! Rules of the writer:
//! - An operation that fails (`Err`) must fail *before* it writes anything. Its caller gets the error,
//!   and the other operations in the batch go on.
//! - A commit that fails fails every operation in its batch with [`StoreError::Failed`], and the writer
//!   then refuses everything until the process starts again: a flush that failed once may report
//!   success later without having written anything.
//! - No write transaction is held across an `.await`; the writer is a plain thread.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::task::{Context, Poll};
use std::thread::{self, JoinHandle};

use redb::WriteTransaction;
use tokio::sync::oneshot;

use crate::{Store, StoreError};

/// The most operations one commit takes. Bounds a batch's latency and its transaction's memory.
const MAX_BATCH: usize = 1024;

/// An operation on the write transaction of a batch.
pub type Op = Box<dyn FnOnce(&WriteTransaction) -> Result<(), StoreError> + Send>;

type Reply = oneshot::Sender<Result<(), StoreError>>;

struct Job {
    op: Op,
    reply: Reply,
}

/// The answer to a submitted operation: resolves once its commit has been flushed, or has failed.
/// A future, so an async caller awaits it; [`Ack::wait`] blocks a thread instead.
#[derive(Debug)]
pub struct Ack(oneshot::Receiver<Result<(), StoreError>>);

impl Future for Ack {
    type Output = Result<(), StoreError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.0).poll(cx) {
            Poll::Ready(Ok(result)) => Poll::Ready(result),
            // The writer went away without answering.
            Poll::Ready(Err(_)) => Poll::Ready(Err(StoreError::Stopped)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Ack {
    /// Blocks the calling thread until the answer is there. Not for use inside an async task.
    ///
    /// # Errors
    /// The operation's own error, a failed commit, or the writer having stopped.
    pub fn wait(self) -> Result<(), StoreError> {
        self.0.blocking_recv().unwrap_or(Err(StoreError::Stopped))
    }
}

/// The writer of one [`Store`].
#[derive(Debug)]
pub struct GroupCommit {
    jobs: Option<mpsc::Sender<Job>>,
    thread: Option<JoinHandle<()>>,
    commits: Arc<AtomicU64>,
}

impl GroupCommit {
    /// Starts the writer thread on `store`. Reads go through `store` as before; writes go through here.
    ///
    /// # Errors
    /// The thread could not be started.
    pub fn start(store: Store) -> Result<Self, StoreError> {
        let (jobs, queue) = mpsc::channel();
        let commits = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&commits);
        let thread = thread::Builder::new()
            .name("doze-store-writer".into())
            .spawn(move || run(&store, &queue, &counter))?;
        Ok(Self {
            jobs: Some(jobs),
            thread: Some(thread),
            commits,
        })
    }

    /// Queues `op`. The returned [`Ack`] resolves after the commit holding it has been flushed.
    pub fn submit(&self, op: Op) -> Ack {
        let (reply, answer) = oneshot::channel();
        match &self.jobs {
            Some(jobs) => {
                if let Err(mpsc::SendError(job)) = jobs.send(Job { op, reply }) {
                    respond(job.reply, Err(StoreError::Stopped));
                }
            }
            None => respond(reply, Err(StoreError::Stopped)),
        }
        Ack(answer)
    }

    /// How many commits (flushes) the writer has made.
    #[must_use]
    pub fn commits(&self) -> u64 {
        self.commits.load(Ordering::Relaxed)
    }

    /// Stops accepting operations, finishes the queued ones, and waits for the thread.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        // Dropping the sender ends the writer's loop once the queue is empty.
        self.jobs = None;
        if let Some(thread) = self.thread.take() {
            // WHY: a join error means an operation panicked and the writer died; its callers saw `Stopped`.
            drop(thread.join());
        }
    }
}

impl Drop for GroupCommit {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Answers a caller. A caller that stopped waiting dropped its receiver; its operation was applied (or
/// refused) all the same, so there is nobody to tell.
fn respond(reply: Reply, result: Result<(), StoreError>) {
    if reply.send(result).is_err() {
        // WHY: nothing to do; see above.
    }
}

fn run(store: &Store, queue: &mpsc::Receiver<Job>, commits: &AtomicU64) {
    let mut failed: Option<String> = None;
    while let Ok(first) = queue.recv() {
        let mut batch = vec![first];
        while batch.len() < MAX_BATCH {
            match queue.try_recv() {
                Ok(job) => batch.push(job),
                Err(_) => break,
            }
        }
        match &failed {
            Some(why) => refuse(batch, why),
            None => failed = commit(store, batch, commits).err(),
        }
    }
}

fn refuse(batch: Vec<Job>, why: &str) {
    for job in batch {
        respond(job.reply, Err(StoreError::Failed(why.to_owned())));
    }
}

/// Applies a batch in one transaction. `Err` carries why the commit failed.
fn commit(store: &Store, batch: Vec<Job>, commits: &AtomicU64) -> Result<(), String> {
    let txn = match store.begin() {
        Ok(txn) => txn,
        Err(e) => {
            let why = e.to_string();
            refuse(batch, &why);
            return Err(why);
        }
    };
    let results: Vec<(Reply, Result<(), StoreError>)> = batch
        .into_iter()
        .map(|job| ((job.reply), (job.op)(&txn)))
        .collect();
    match txn.commit() {
        Ok(()) => {
            commits.fetch_add(1, Ordering::Relaxed);
            for (reply, result) in results {
                respond(reply, result);
            }
            Ok(())
        }
        Err(e) => {
            let why = e.to_string();
            for (reply, _) in results {
                respond(reply, Err(StoreError::Failed(why.clone())));
            }
            Err(why)
        }
    }
}

#[cfg(test)]
mod tests;
