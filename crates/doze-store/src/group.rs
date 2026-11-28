//! Group commit: one writer thread per data directory (doze-aws has one) owns the writes of every open database in it. It takes
//! whatever is queued, applies each database's share in one transaction of that database and flushes it once.
//!
//! Every caller still gets a durable answer: its [`Ack`] resolves only after the commit that holds its operation has been flushed.
//! Under load many callers share a flush (spike S6 measured p50 about 6 ms and p99 under 15 ms for durable sends at 10,000 a second
//! on one core); alone, a caller pays one flush.
//!
//! Rules of the writer:
//! - An operation that fails (`Err`) must fail *before* it writes anything. Its caller gets the error, and the other operations in
//!   the batch go on.
//! - A commit that fails fails every operation in its batch with [`StoreError::Failed`], and that database's writes are then
//!   refused until the process starts again: a flush that failed once may report success later without having written anything.
//!   The other databases go on.
//! - No write transaction is held across an `.await`; the writer is a plain thread.
//!
//! One thread, not one per database: seventeen services each held a writer thread (and on Linux a malloc arena each, about 210 KB)
//! while open. The thread starts with the directory's first database and ends when its last one closes, so a process whose services
//! are all idle holds none (weight.md, "Workstream 2"). Per data directory, not per process, so a test that runs many servers at
//! once does not queue one server's flushes behind another's.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};

use redb::WriteTransaction;
use tokio::sync::oneshot;

use crate::pending::{Overlaid, Pending};
use crate::{Store, StoreError};

mod writer;

/// An operation on the write transaction of a batch.
pub type Op = Box<dyn FnOnce(&WriteTransaction) -> Result<(), StoreError> + Send>;

type Reply = oneshot::Sender<Result<(), StoreError>>;

struct Job {
    op: Op,
    reply: Reply,
    /// Where the job's writes are laid over the file until its commit ([`Pending`]), and their number there.
    seen: Option<(Pending, u64)>,
}

impl Job {
    /// Its commit failed, or it was refused: nothing queued for the database is laid over the file any more.
    fn failed(&self) {
        failed(self.seen.as_ref());
    }
}

/// A job's writes are on disk, or its operation failed alone: a read no longer lays them over the file.
fn settled(seen: Option<&(Pending, u64)>) {
    if let Some((pending, id)) = seen {
        pending.done(*id);
    }
}

/// See [`Job::failed`].
fn failed(seen: Option<&(Pending, u64)>) {
    if let Some((pending, _)) = seen {
        pending.fail();
    }
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

/// The writes of one [`Store`], on the writer thread of its data directory.
#[derive(Debug)]
pub struct GroupCommit {
    /// The database on its writer; `None` once stopped.
    link: Option<writer::Link>,
    commits: Arc<AtomicU64>,
    /// What was submitted with [`GroupCommit::submit_seen`] and is not yet committed.
    pending: Pending,
}

impl GroupCommit {
    /// Hands `store` to the writer thread of its data directory (starting it if none of the directory's databases is open). Reads go through `store` as before; writes go
    /// through here.
    ///
    /// # Errors
    /// The thread could not be started.
    pub fn start(store: Store) -> Result<Self, StoreError> {
        let commits = Arc::new(AtomicU64::new(0));
        let link = writer::open(store, Arc::clone(&commits))?;
        Ok(Self {
            link: Some(link),
            commits,
            pending: Pending::default(),
        })
    }

    /// Queues `op`. The returned [`Ack`] resolves after the commit holding it has been flushed.
    pub fn submit(&self, op: Op) -> Ack {
        self.queue(op, None)
    }

    /// Queues `op`, whose writes are `seen`: until its commit, [`GroupCommit::pending`] lays them over the file for a read.
    pub fn submit_seen(&self, op: Op, seen: Arc<dyn Overlaid>) -> Ack {
        let id = self.pending.add(seen);
        self.queue(op, id.map(|id| (self.pending.clone(), id)))
    }

    fn queue(&self, op: Op, seen: Option<(Pending, u64)>) -> Ack {
        let (reply, answer) = oneshot::channel();
        let job = Job { op, reply, seen };
        match &self.link {
            Some(link) => link.submit(job),
            None => {
                job.failed();
                respond(job.reply, Err(StoreError::Stopped));
            }
        }
        Ack(answer)
    }

    /// The writes queued with [`GroupCommit::submit_seen`] and not yet committed.
    #[must_use]
    pub fn pending(&self) -> &Pending {
        &self.pending
    }

    /// How many commits (flushes) the writer has made of this database.
    #[must_use]
    pub fn commits(&self) -> u64 {
        self.commits.load(Ordering::Relaxed)
    }

    /// Finishes what is queued, takes the database off the writer, runs `f` with `store` as the only handle (so it may
    /// [`Store::compact`]), and hands it to the writer again. Nothing can be submitted meanwhile: the caller holds `&mut self`.
    ///
    /// # Errors
    /// The writer's thread could not be started again; `f`'s own result is returned inside.
    pub fn paused<T>(
        &mut self,
        store: &mut Store,
        f: impl FnOnce(&mut Store) -> T,
    ) -> Result<T, StoreError> {
        self.stop();
        let out = f(store);
        self.link = Some(writer::open(store.clone(), Arc::clone(&self.commits))?);
        self.pending.revive();
        Ok(out)
    }

    /// [`GroupCommit::paused`] on a thread of the blocking pool: waiting out the queued commits, `f` (a compaction: whole seconds on
    /// a large file) and the restart all run there, and the caller awaits them. `store` is moved there and handed back; `None` when
    /// the runtime stopped before the job could run (the database is then closed with the job).
    ///
    /// # Errors
    /// The writer's thread could not be started again (the store is handed back all the same); `f`'s own result is returned inside.
    pub async fn paused_off_loop<T: Send + 'static>(
        &mut self,
        store: Store,
        f: impl FnOnce(&mut Store) -> T + Send + 'static,
    ) -> Option<(Store, Result<T, StoreError>)> {
        let link = self.link.take();
        let commits = Arc::clone(&self.commits);
        let job = tokio::task::spawn_blocking(move || {
            if let Some(link) = link {
                link.close().wait();
            }
            let mut store = store;
            let out = f(&mut store);
            let link = writer::open(store.clone(), commits);
            (store, out, link)
        });
        match job.await {
            Ok((store, out, Ok(link))) => {
                self.link = Some(link);
                self.pending.revive();
                Some((store, Ok(out)))
            }
            Ok((store, _, Err(e))) => Some((store, Err(e))),
            Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
            Err(_) => None,
        }
    }

    /// Stops accepting operations, and waits until the writer has committed this database's queued ones and let go of it. Waits
    /// for this database alone: the thread goes on with the others.
    pub fn shutdown(mut self) {
        self.stop();
    }

    /// How many writer threads run: one per data directory with a database open (for tests).
    #[doc(hidden)]
    #[must_use]
    pub fn writer_threads() -> usize {
        writer::running()
    }

    fn stop(&mut self) {
        if let Some(link) = self.link.take() {
            link.close().wait();
        }
    }
}

impl Drop for GroupCommit {
    /// Waits, as [`GroupCommit::shutdown`] does: the database must be let go of before its file can be opened again.
    fn drop(&mut self) {
        self.stop();
    }
}

/// Answers a caller. A caller that stopped waiting dropped its receiver; its operation was applied (or refused) all the same, so
/// there is nobody to tell.
fn respond(reply: Reply, result: Result<(), StoreError>) {
    if reply.send(result).is_err() {
        // WHY: nothing to do; see above.
    }
}

#[cfg(test)]
mod tests;
