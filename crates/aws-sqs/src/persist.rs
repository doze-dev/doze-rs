//! SQS on disk.
//!
//! One redb file (`sqs.redb`) holds four tables:
//! - `queues`: a queue's header (settings, tags, times), keyed by name;
//! - `fifo`: a FIFO queue's bookkeeping (sequence, deduplication window, last receive attempt), keyed by name;
//! - `messages`: one row per message, keyed by queue name and the message's `ord`, so a send, a receive and a delete each
//!   write one row and a queue of a million messages costs no more per call than one of ten;
//! - `meta`: the recently deleted queue names and the move tasks.
//!
//! A call changes the state in memory (the state machine knows nothing of the disk); after it, [`Core::flush`] finds what
//! changed and hands the new rows to the group-commit writer. Messages say what changed themselves (see
//! [`crate::model::Messages`]); a header, a FIFO state and the meta are compared with the bytes last written, so a change
//! cannot be made and forgotten. The reply is held back until the commit is flushed: an acknowledged send survives a crash.

use std::collections::BTreeMap;
use std::path::Path;

use doze_fs::fs::{Fs, RealFs};
use doze_store::{Ack, GroupCommit, Op, Store, StoreError, record};
use redb::{ReadableDatabase, ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::core::Core;
use crate::fifo::FifoState;
use crate::model::{Attrs, Message, Messages, Queue};
use crate::moves::Task;
use crate::queues::WAIT_MS;

const QUEUES: TableDefinition<&str, &[u8]> = TableDefinition::new("queues");
const FIFO: TableDefinition<&str, &[u8]> = TableDefinition::new("fifo");
const MESSAGES: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("messages");
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");
const META_KEY: &str = "state";

const FILE: &str = "sqs.redb";
/// redb's page cache: a dev tool's queues are small, and the idle heap is measured.
const CACHE_BYTES: usize = 1 << 20;
/// The version of every record here. A change of any row's shape is a new version with a branch that reads the old one.
const VERSION: u8 = 1;

/// The bytes last written for a queue, so an unchanged header or FIFO state is not written again.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Written {
    header: Vec<u8>,
    fifo: Vec<u8>,
}

/// A queue's header as it is written. [`Header`] reads it back: the two have the same fields in the same order.
#[derive(Serialize)]
struct HeaderRef<'a> {
    attrs: &'a Attrs,
    stored: &'a BTreeMap<String, String>,
    created_secs: i64,
    modified_secs: i64,
    tags: &'a BTreeMap<String, String>,
    purged_ms: Option<i64>,
}

#[derive(Deserialize)]
struct Header {
    attrs: Attrs,
    stored: BTreeMap<String, String>,
    created_secs: i64,
    modified_secs: i64,
    tags: BTreeMap<String, String>,
    purged_ms: Option<i64>,
}

#[derive(Serialize)]
struct MetaRef<'a> {
    deleted: &'a BTreeMap<String, i64>,
    tasks: &'a [Task],
}

#[derive(Deserialize)]
struct Meta {
    deleted: BTreeMap<String, i64>,
    tasks: Vec<Task>,
}

/// What a database holds, as the core wants it.
pub(crate) struct Loaded {
    pub(crate) queues: BTreeMap<String, Queue>,
    pub(crate) deleted: BTreeMap<String, i64>,
    pub(crate) tasks: Vec<Task>,
    pub(crate) written_meta: Vec<u8>,
}

/// The open database and its writer.
#[derive(Debug)]
pub(crate) struct Persist {
    store: Store,
    writer: GroupCommit,
}

impl Persist {
    /// Opens (or makes) the database in `dir` and reads everything in it.
    pub(crate) fn open(dir: &Path) -> Result<(Self, Loaded), StoreError> {
        let fs = RealFs;
        fs.create_dir_all(dir)?;
        let store = Store::open(&fs, dir, FILE, CACHE_BYTES, |txn| {
            txn.open_table(QUEUES)?;
            txn.open_table(FIFO)?;
            txn.open_table(MESSAGES)?;
            txn.open_table(META)?;
            Ok(())
        })?;
        let loaded = load(&store)?;
        let writer = GroupCommit::start(store.clone())?;
        Ok((Self { store, writer }, loaded))
    }

    /// Stops the writer after it has finished what it was given, and closes the database.
    pub(crate) fn close(self) {
        self.writer.shutdown();
        // WHY: every commit was flushed when it was made, so a failure to close costs only a longer start next time.
        drop(self.store.close());
    }
}

fn load(store: &Store) -> Result<Loaded, StoreError> {
    let read = store.begin_read()?;
    let mut rows: BTreeMap<String, Vec<Message>> = BTreeMap::new();
    for row in read.open_table(MESSAGES)?.iter()? {
        let (key, bytes) = row?;
        let message: Message = record::decode("message", VERSION, bytes.value())?;
        rows.entry(key.value().0.to_owned())
            .or_default()
            .push(message);
    }
    let mut fifos: BTreeMap<String, (FifoState, Vec<u8>)> = BTreeMap::new();
    for row in read.open_table(FIFO)?.iter()? {
        let (name, bytes) = row?;
        let state = record::decode("fifo state", VERSION, bytes.value())?;
        fifos.insert(name.value().to_owned(), (state, bytes.value().to_vec()));
    }
    let mut queues = BTreeMap::new();
    for row in read.open_table(QUEUES)?.iter()? {
        let (name, bytes) = row?;
        let header: Header = record::decode("queue header", VERSION, bytes.value())?;
        let name = name.value().to_owned();
        let messages = rows.remove(&name).unwrap_or_default();
        // The next ord is one past the last: an ord freed by a delete may be given again, because its row is gone.
        let next_ord = messages.last().map_or(0, |m| m.ord + 1);
        let (fifo, fifo_bytes) = fifos.remove(&name).unwrap_or_default();
        let queue = Queue {
            name: name.clone(),
            attrs: header.attrs,
            stored: header.stored,
            created_secs: header.created_secs,
            modified_secs: header.modified_secs,
            messages: Messages::restored(messages, next_ord),
            tags: header.tags,
            fifo,
            purged_ms: header.purged_ms,
            written: Written {
                header: bytes.value().to_vec(),
                fifo: fifo_bytes,
            },
        };
        queues.insert(name, queue);
    }
    let meta_table = read.open_table(META)?;
    let (deleted, tasks, written_meta) = match meta_table.get(META_KEY)? {
        Some(bytes) => {
            let meta: Meta = record::decode("meta", VERSION, bytes.value())?;
            (meta.deleted, meta.tasks, bytes.value().to_vec())
        }
        None => (BTreeMap::new(), Vec::new(), Vec::new()),
    };
    Ok(Loaded {
        queues,
        deleted,
        tasks,
        written_meta,
    })
}

/// The rows one call changed.
#[derive(Default)]
pub(crate) struct Writes {
    dropped: Vec<String>,
    headers: Vec<(String, Vec<u8>)>,
    fifos: Vec<(String, Vec<u8>)>,
    messages: Vec<(String, u64, Vec<u8>)>,
    removed: Vec<(String, u64)>,
    meta: Option<Vec<u8>>,
}

impl Writes {
    /// How many rows are written or deleted.
    #[cfg(test)]
    pub(crate) fn rows(&self) -> usize {
        self.dropped.len()
            + self.headers.len()
            + self.fifos.len()
            + self.messages.len()
            + self.removed.len()
            + usize::from(self.meta.is_some())
    }

    fn is_empty(&self) -> bool {
        self.dropped.is_empty()
            && self.headers.is_empty()
            && self.fifos.is_empty()
            && self.messages.is_empty()
            && self.removed.is_empty()
            && self.meta.is_none()
    }

    /// All of it, in the one transaction of the batch it lands in.
    fn apply(&self, txn: &WriteTransaction) -> Result<(), StoreError> {
        for name in &self.dropped {
            txn.open_table(QUEUES)?.remove(name.as_str())?;
            txn.open_table(FIFO)?.remove(name.as_str())?;
            txn.open_table(MESSAGES)?
                .retain_in((name.as_str(), 0)..=(name.as_str(), u64::MAX), |_, _| false)?;
        }
        let mut queues = txn.open_table(QUEUES)?;
        for (name, bytes) in &self.headers {
            queues.insert(name.as_str(), bytes.as_slice())?;
        }
        let mut fifo = txn.open_table(FIFO)?;
        for (name, bytes) in &self.fifos {
            fifo.insert(name.as_str(), bytes.as_slice())?;
        }
        let mut messages = txn.open_table(MESSAGES)?;
        for (name, ord, bytes) in &self.messages {
            messages.insert((name.as_str(), *ord), bytes.as_slice())?;
        }
        for (name, ord) in &self.removed {
            messages.remove((name.as_str(), *ord))?;
        }
        if let Some(bytes) = &self.meta {
            txn.open_table(META)?.insert(META_KEY, bytes.as_slice())?;
        }
        Ok(())
    }
}

impl Core {
    /// Sends what the last call changed to the writer. The returned [`Ack`] resolves when it is on disk; `None` when nothing
    /// changed (or there is no disk).
    pub(crate) fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let writes = self.stage()?;
        let Some(persist) = &self.persist else {
            return Ok(None);
        };
        if writes.is_empty() {
            return Ok(None);
        }
        let op: Op = Box::new(move |txn| writes.apply(txn));
        Ok(Some(persist.writer.submit(op)))
    }

    /// What the last call changed, as rows; the state is marked as written.
    pub(crate) fn stage(&mut self) -> Result<Writes, StoreError> {
        let mut writes = Writes {
            dropped: std::mem::take(&mut self.removed),
            ..Writes::default()
        };
        let touched = std::mem::take(&mut self.touched);
        for (name, queue) in &mut self.queues {
            if touched.contains(name) {
                stage_queue(name, queue, &mut writes)?;
            }
            if queue.messages.has_changes() {
                for message in queue.messages.to_write() {
                    let bytes = record::encode("message", VERSION, message)?;
                    writes.messages.push((name.clone(), message.ord, bytes));
                }
                writes
                    .removed
                    .extend(queue.messages.to_delete().map(|ord| (name.clone(), ord)));
                queue.messages.settle();
            }
        }
        self.stage_meta(&mut writes)?;
        Ok(writes)
    }

    fn stage_meta(&mut self, writes: &mut Writes) -> Result<(), StoreError> {
        let now = self.now_ms();
        self.deleted.retain(|_, at| now - *at < WAIT_MS);
        if self.deleted.is_empty() && self.tasks.is_empty() && self.written_meta.is_empty() {
            return Ok(());
        }
        let bytes = record::encode(
            "meta",
            VERSION,
            &MetaRef {
                deleted: &self.deleted,
                tasks: &self.tasks,
            },
        )?;
        if bytes != self.written_meta {
            self.written_meta.clone_from(&bytes);
            writes.meta = Some(bytes);
        }
        Ok(())
    }

    /// A core over what a database held (or over nothing, when `loaded` is `None`).
    pub(crate) fn restore(
        clock: std::sync::Arc<dyn doze_runtime::Clock>,
        persist: Option<(Persist, Loaded)>,
    ) -> Self {
        let mut core = Self::new(clock);
        if let Some((persist, loaded)) = persist {
            core.queues = loaded.queues;
            core.deleted = loaded.deleted;
            core.tasks = loaded.tasks;
            core.written_meta = loaded.written_meta;
            core.persist = Some(persist);
        }
        core
    }
}

fn stage_queue(name: &str, queue: &mut Queue, writes: &mut Writes) -> Result<(), StoreError> {
    let header = record::encode(
        "queue header",
        VERSION,
        &HeaderRef {
            attrs: &queue.attrs,
            stored: &queue.stored,
            created_secs: queue.created_secs,
            modified_secs: queue.modified_secs,
            tags: &queue.tags,
            purged_ms: queue.purged_ms,
        },
    )?;
    if header != queue.written.header {
        queue.written.header.clone_from(&header);
        writes.headers.push((name.to_owned(), header));
    }
    if queue.is_fifo() {
        let fifo = record::encode("fifo state", VERSION, &queue.fifo)?;
        if fifo != queue.written.fifo {
            queue.written.fifo.clone_from(&fifo);
            writes.fifos.push((name.to_owned(), fifo));
        }
    }
    Ok(())
}
