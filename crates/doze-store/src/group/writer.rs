//! The writer thread of a data directory, and the queue to it.
//!
//! One thread writes every database under a data directory: doze-aws has one, so it has one writer however many services are
//! open. (A test that runs many servers at once in one process gives each its own, so their flushes do not queue behind each
//! other's.) Each round takes what is queued (up to [`MAX_BATCH`] operations), groups it by database in the order it arrived, and
//! commits each database's share in one transaction. A database is handed over by [`open`] and taken back by [`Link::close`], which
//! is answered once the operations queued before it are committed and the writer has let go of the database. The thread starts
//! with the first database of its directory and ends after the last one closes.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc};
use std::thread;

use super::{Job, failed, respond, settled};
use crate::{Store, StoreError};

/// The most operations one round takes. Bounds a batch's latency and its transactions' memory.
const MAX_BATCH: usize = 1024;

enum Msg {
    Open {
        id: u64,
        store: Store,
        commits: Arc<AtomicU64>,
    },
    Job {
        id: u64,
        job: Job,
    },
    Close {
        id: u64,
        done: mpsc::Sender<()>,
    },
}

/// A running writer: its queue, and how many databases are on it.
struct Hub {
    data: PathBuf,
    queue: mpsc::Sender<Msg>,
    live: usize,
}

static HUBS: Mutex<Vec<Hub>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn hubs() -> MutexGuard<'static, Vec<Hub>> {
    HUBS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How many writer threads are running (for tests).
pub(super) fn running() -> usize {
    hubs().len()
}

/// The data directory a database file belongs to: the one above its service's directory, a region's directory
/// (`<service>/regions/<region>`) counting as its service's.
fn data_dir_of(file: &Path) -> PathBuf {
    let mut dir = file.parent().unwrap_or(file);
    if dir.parent().and_then(Path::file_name) == Some("regions".as_ref()) {
        dir = dir.parent().and_then(Path::parent).unwrap_or(dir);
    }
    dir.parent().unwrap_or(dir).to_path_buf()
}

/// A database on its writer.
#[derive(Debug)]
pub(super) struct Link {
    data: PathBuf,
    id: u64,
    queue: mpsc::Sender<Msg>,
}

/// Hands `store` to the writer of its data directory, starting the thread if none runs.
pub(super) fn open(store: Store, commits: Arc<AtomicU64>) -> Result<Link, StoreError> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let data = data_dir_of(store.path());
    let mut hubs = hubs();
    let mut msg = Msg::Open { id, store, commits };
    // Twice at most: a thread that is gone (it panicked past its guard) is replaced once.
    for _ in 0..2 {
        let at = match hubs.iter().position(|h| h.data == data) {
            Some(at) => at,
            None => {
                let (queue, taken) = mpsc::channel();
                thread::Builder::new()
                    .name("doze-store-writer".into())
                    .spawn(move || run(&taken))?;
                hubs.push(Hub {
                    data: data.clone(),
                    queue,
                    live: 0,
                });
                hubs.len() - 1
            }
        };
        let hub = hubs.get_mut(at).ok_or(StoreError::Stopped)?;
        match hub.queue.send(msg) {
            Ok(()) => {
                hub.live += 1;
                let queue = hub.queue.clone();
                return Ok(Link { data, id, queue });
            }
            Err(mpsc::SendError(back)) => {
                hubs.swap_remove(at);
                msg = back;
            }
        }
    }
    Err(StoreError::Stopped)
}

impl Link {
    /// Queues an operation of this database.
    pub(super) fn submit(&self, job: Job) {
        let id = self.id;
        if let Err(mpsc::SendError(Msg::Job { job, .. })) = self.queue.send(Msg::Job { id, job }) {
            job.failed();
            respond(job.reply, Err(StoreError::Stopped));
        }
    }

    /// Takes the database off the writer after what was queued for it; the last one of its directory to go ends the thread.
    pub(super) fn close(self) -> Closing {
        let (done, closed) = mpsc::channel();
        let sent = self.queue.send(Msg::Close { id: self.id, done }).is_ok();
        // The thread ends once every sender is gone: this one, and the hub's when this was its last database.
        drop(self.queue);
        let mut hubs = hubs();
        if let Some(at) = hubs.iter().position(|h| h.data == self.data)
            && let Some(hub) = hubs.get_mut(at)
        {
            hub.live = hub.live.saturating_sub(1);
            if hub.live == 0 {
                hubs.swap_remove(at);
            }
        }
        Closing(sent.then_some(closed))
    }
}

/// What a closing database waits on.
pub(super) struct Closing(Option<mpsc::Receiver<()>>);

impl Closing {
    /// Blocks until the writer has committed what was queued before the close and let go of the database.
    pub(super) fn wait(self) {
        if let Some(done) = self.0 {
            // WHY: an error means the thread is gone, which lets go of the database just as well.
            done.recv().ok();
        }
    }
}

/// A database on the writer.
struct Open {
    store: Store,
    commits: Arc<AtomicU64>,
    /// Why a commit failed, after which its writes are refused.
    failed: Option<String>,
}

fn run(queue: &mpsc::Receiver<Msg>) {
    let mut open: HashMap<u64, Open> = HashMap::new();
    while let Ok(first) = queue.recv() {
        let mut batches: Vec<(u64, Vec<Job>)> = Vec::new();
        let mut taken = 0;
        let mut next = Some(first);
        while let Some(msg) = next {
            match msg {
                Msg::Open { id, store, commits } => {
                    let failed = None;
                    open.insert(
                        id,
                        Open {
                            store,
                            commits,
                            failed,
                        },
                    );
                }
                Msg::Job { id, job } => {
                    taken += 1;
                    match batches.iter_mut().find(|(of, _)| *of == id) {
                        Some((_, jobs)) => jobs.push(job),
                        None => batches.push((id, vec![job])),
                    }
                }
                Msg::Close { id, done } => {
                    if let Some(at) = batches.iter().position(|(of, _)| *of == id) {
                        let (_, jobs) = batches.remove(at);
                        commit_to(&mut open, id, jobs);
                    }
                    open.remove(&id);
                    // WHY: a closer that stopped waiting needs no answer.
                    done.send(()).ok();
                }
            }
            next = if taken < MAX_BATCH {
                queue.try_recv().ok()
            } else {
                None
            };
        }
        for (id, jobs) in batches {
            commit_to(&mut open, id, jobs);
        }
    }
}

fn commit_to(open: &mut HashMap<u64, Open>, id: u64, jobs: Vec<Job>) {
    let Some(db) = open.get_mut(&id) else {
        for job in jobs {
            job.failed();
            respond(job.reply, Err(StoreError::Stopped));
        }
        return;
    };
    match &db.failed {
        Some(why) => refuse(jobs, why),
        None => db.failed = commit(&db.store, jobs, &db.commits).err(),
    }
}

fn refuse(batch: Vec<Job>, why: &str) {
    for job in batch {
        job.failed();
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
    let results: Vec<_> = batch
        .into_iter()
        .map(|job| {
            let Job { op, reply, seen } = job;
            // WHY: the thread serves every database; an operation that panics fails alone (a release build aborts anyway).
            let done = catch_unwind(AssertUnwindSafe(|| op(&txn)))
                .unwrap_or_else(|_| Err(StoreError::Failed("the operation panicked".into())));
            (reply, seen, done)
        })
        .collect();
    match txn.commit() {
        Ok(()) => {
            commits.fetch_add(1, Ordering::Relaxed);
            // Each entry leaves the overlay after the commit returned (so a read begun now sees it in the file), and before its
            // caller hears: an operation that failed alone wrote nothing, and leaves it too.
            for (reply, seen, result) in results {
                settled(seen.as_ref());
                respond(reply, result);
            }
            Ok(())
        }
        Err(e) => {
            let why = e.to_string();
            for (reply, seen, _) in results {
                failed(seen.as_ref());
                respond(reply, Err(StoreError::Failed(why.clone())));
            }
            Err(why)
        }
    }
}
