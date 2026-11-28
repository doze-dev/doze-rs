//! The file: one redb database per region with eight tables (the table records, the Contributor Insights settings, the other
//! records of `extras.rs`, the items, the index entries, the stream records, the items of backups, the point-in-time recovery log),
//! written through the group-commit writer. Items are read from the file when a request needs them; nothing but the table
//! records is kept in memory.

use std::path::Path;

use doze_fs::fs::{Fs, RealFs};
use std::sync::Arc;

use doze_store::pending::{Change, Overlaid, Span};
use doze_store::records::Changes;
use doze_store::{Ack, GroupCommit, Op, Store, StoreError};
use redb::{ReadableDatabase, TableDefinition};

use crate::keys::Space;

/// redb's page cache: items are read when needed, and the idle heap is measured.
const CACHE_BYTES: usize = 1 << 20;
pub(crate) const FILE: &str = "dynamodb.redb";
pub(crate) const TABLES: &str = "tables";
/// The Contributor Insights settings, by table (and `table/index`).
pub(crate) const INSIGHTS: &str = "insights";
/// Backups, exports, imports, resource policies and Kinesis streaming destinations (`extras.rs`).
pub(crate) const EXTRAS: &str = "extras";

type Def = TableDefinition<'static, &'static [u8], &'static [u8]>;

pub(crate) fn def(space: Space) -> Def {
    TableDefinition::new(space.table_name())
}

/// The file and its writer.
#[derive(Debug)]
pub(crate) struct Disk {
    /// `None` only while [`Disk::compact_off_loop`] has it on a blocking thread (the actor waits then), or after the runtime stopped
    /// during one.
    store: Option<Store>,
    writer: GroupCommit,
    /// How many writes were handed to the writer: a count made at one value is still exact while it holds.
    writes: std::sync::atomic::AtomicU64,
    /// Counts of entries and bytes under a prefix, made at the write count beside each (`sizes.rs`).
    pub(crate) counted: std::collections::HashMap<(Space, Vec<u8>), (u64, crate::sizes::Count)>,
}

/// Everything one call changes, applied in one transaction.
#[derive(Debug, Default)]
pub(crate) struct Writes {
    pub(crate) meta: Vec<Changes>,
    pub(crate) puts: Vec<(Space, Vec<u8>, Vec<u8>)>,
    pub(crate) dels: Vec<(Space, Vec<u8>)>,
    /// Every entry from the first key up to (not including) the second.
    pub(crate) ranges: Vec<(Space, Vec<u8>, Vec<u8>)>,
    /// Work done inside the transaction itself, after the rest, on the writer's thread: a copy of many entries (a backup, a
    /// restore) that would otherwise be read and staged on the event loop.
    pub(crate) ops: Vec<TxnOp>,
}

/// Work done inside a write transaction: it reads what the transaction sees (every write before it) and writes into it.
pub(crate) struct TxnOp(pub(crate) Op);

impl std::fmt::Debug for TxnOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TxnOp")
    }
}

/// The writes of [`Writes`] a read can see while they are queued: all but the work inside the transaction ([`Writes::ops`]), which is
/// why a call with such work waits for its commit ([`crate::core::Core::save`]).
struct Queued {
    meta: Vec<Changes>,
    puts: Vec<(Space, Vec<u8>, Vec<u8>)>,
    dels: Vec<(Space, Vec<u8>)>,
    ranges: Vec<(Space, Vec<u8>, Vec<u8>)>,
}

impl Overlaid for Queued {
    /// In the order [`Disk::submit`] applies them.
    fn changes(&self, table: &str, f: &mut dyn FnMut(Change<'_>)) {
        for c in &self.meta {
            c.seen(table, f);
        }
        let ours = |space: &Space| space.table_name() == table;
        for (_, lo, hi) in self.ranges.iter().filter(|(s, ..)| ours(s)) {
            f(Change::DeleteRange(lo, hi));
        }
        for (_, key) in self.dels.iter().filter(|(s, _)| ours(s)) {
            f(Change::Delete(key));
        }
        for (_, key, value) in self.puts.iter().filter(|(s, ..)| ours(s)) {
            f(Change::Put(key, value));
        }
    }
}

impl Writes {
    pub(crate) fn is_empty(&self) -> bool {
        self.meta.iter().all(Changes::is_empty)
            && self.puts.is_empty()
            && self.dels.is_empty()
            && self.ranges.is_empty()
            && self.ops.is_empty()
    }

    pub(crate) fn put(&mut self, space: Space, key: Vec<u8>, value: Vec<u8>) {
        self.puts.push((space, key, value));
    }

    pub(crate) fn del(&mut self, space: Space, key: Vec<u8>) {
        self.dels.push((space, key));
    }
}

/// What to do with the next entry of a read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flow {
    Continue,
    Stop,
}

impl Disk {
    pub(crate) fn open(dir: &Path) -> Result<Self, StoreError> {
        let fs = RealFs;
        fs.create_dir_all(dir)?;
        let store = Store::open(&fs, dir, FILE, CACHE_BYTES, |txn| {
            for meta in [TABLES, INSIGHTS, EXTRAS] {
                txn.open_table(TableDefinition::<&str, &[u8]>::new(meta))?;
            }
            for space in Space::ALL {
                txn.open_table(def(space))?;
            }
            Ok(())
        })?;
        // A file made before Contributor Insights, backups and the recovery log were kept has no tables for them.
        let read = store.begin_read()?;
        let missing_meta = [INSIGHTS, EXTRAS].into_iter().any(|m| {
            read.open_table(TableDefinition::<&str, &[u8]>::new(m))
                .is_err()
        });
        let missing_space = Space::ALL
            .into_iter()
            .any(|s| read.open_table(def(s)).is_err());
        drop(read);
        if missing_meta || missing_space {
            let txn = store.begin()?;
            for meta in [INSIGHTS, EXTRAS] {
                txn.open_table(TableDefinition::<&str, &[u8]>::new(meta))?;
            }
            for space in Space::ALL {
                txn.open_table(def(space))?;
            }
            txn.commit()?;
        }
        let writer = GroupCommit::start(store.clone())?;
        Ok(Self {
            store: Some(store),
            writer,
            writes: std::sync::atomic::AtomicU64::new(0),
            counted: std::collections::HashMap::new(),
        })
    }

    /// The table records, as the rows of the `tables` table.
    pub(crate) fn read(&self) -> Result<redb::ReadTransaction, StoreError> {
        Ok(self.store()?.begin_read()?)
    }

    /// The value filed under `key`, or left there by a write still queued.
    pub(crate) fn get(&self, space: Space, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        if let Some(queued) = self.writer.pending().view().get(space.table_name(), key) {
            return Ok(queued);
        }
        let txn = self.store()?.begin_read()?;
        let table = txn.open_table(def(space))?;
        Ok(table.get(key)?.map(|v| v.value().to_vec()))
    }

    /// Calls `each` with the entries from `lo` up to (not including) `hi`, in order or in reverse, until it says stop: those of the
    /// file with the queued writes laid over them.
    pub(crate) fn scan(
        &self,
        space: Space,
        lo: &[u8],
        hi: &[u8],
        reverse: bool,
        mut each: impl FnMut(&[u8], &[u8]) -> Flow,
    ) -> Result<(), StoreError> {
        if lo >= hi {
            return Ok(());
        }
        // Taken before the read begins (doze_store::pending says why).
        let view = self.writer.pending().view();
        let txn = self.store()?.begin_read()?;
        let table = txn.open_table(def(space))?;
        let range = table.range::<&[u8]>(lo..hi)?;
        if !view.is_empty() {
            let rows: Box<dyn Iterator<Item = _>> = if reverse {
                Box::new(range.rev())
            } else {
                Box::new(range)
            };
            let span = Span {
                table: space.table_name(),
                lo,
                hi,
                reverse,
            };
            return Ok(view.merge(
                span,
                rows,
                |(k, v)| (k.value(), v.value()),
                |k, v| each(k, v) == Flow::Continue,
            )?);
        }
        if reverse {
            for row in range.rev() {
                let (k, v) = row?;
                if each(k.value(), v.value()) == Flow::Stop {
                    break;
                }
            }
        } else {
            for row in range {
                let (k, v) = row?;
                if each(k.value(), v.value()) == Flow::Stop {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Queues the writes. The [`Ack`] resolves when they are on disk, and until then [`Disk::get`] and [`Disk::scan`] see them (all
    /// but the work inside the transaction, [`Writes::ops`]); `None` when there is nothing to write.
    pub(crate) fn submit(&self, writes: Writes) -> Option<Ack> {
        if writes.is_empty() {
            return None;
        }
        let Writes {
            meta,
            puts,
            dels,
            ranges,
            ops,
        } = writes;
        let writes = Arc::new(Queued {
            meta,
            puts,
            dels,
            ranges,
        });
        let seen = Arc::clone(&writes);
        let op: Op = Box::new(move |txn| {
            for c in &writes.meta {
                c.apply(txn)?;
            }
            for (space, lo, hi) in &writes.ranges {
                let mut table = txn.open_table(def(*space))?;
                table.retain_in::<&[u8], _>(lo.as_slice()..hi.as_slice(), |_, _| false)?;
            }
            for (space, key) in &writes.dels {
                txn.open_table(def(*space))?.remove(key.as_slice())?;
            }
            for (space, key, value) in &writes.puts {
                txn.open_table(def(*space))?
                    .insert(key.as_slice(), value.as_slice())?;
            }
            for TxnOp(op) in ops {
                op(txn)?;
            }
            Ok(())
        });
        self.writes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Some(self.writer.submit_seen(op, seen))
    }

    /// Holds the writer until the sender is sent to or dropped: what is submitted meanwhile stays queued (for tests).
    #[cfg(test)]
    pub(crate) fn hold(&self) -> (std::sync::mpsc::Sender<()>, Ack) {
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let (started, running) = std::sync::mpsc::channel::<()>();
        let ack = self.writer.submit(Box::new(move |_| {
            started.send(()).ok();
            wait.recv().ok();
            Ok(())
        }));
        running.recv().ok();
        (release, ack)
    }

    /// Whether a write is queued and not yet committed (for tests).
    #[cfg(test)]
    pub(crate) fn queued(&self) -> bool {
        !self.writer.pending().is_empty()
    }

    /// Waits until every write queued so far is committed: for a read the overlay cannot reach (a count off the serving thread).
    pub(crate) async fn settled(&self) {
        self.writer.pending().settled().await;
    }

    /// The entries from `lo` up to (not including) `hi`, and the bytes their keys and values hold: the count made off the serving
    /// thread when there is one since the last write (`sizes.rs`), else read here.
    pub(crate) fn measure(
        &self,
        space: Space,
        lo: &[u8],
        hi: &[u8],
    ) -> Result<(u64, u64), StoreError> {
        if let Some(count) = self.counted(space, lo)
            && hi == crate::keys::prefix_end(lo).as_slice()
        {
            return Ok(count);
        }
        let (mut rows, mut bytes) = (0, 0);
        self.scan(space, lo, hi, false, |k, v| {
            rows += 1;
            bytes += (k.len() + v.len()) as u64;
            Flow::Continue
        })?;
        Ok((rows, bytes))
    }

    pub(crate) fn store(&self) -> Result<&Store, StoreError> {
        self.store.as_ref().ok_or(StoreError::Stopped)
    }

    /// How many writes were handed to the writer so far.
    pub(crate) fn writes(&self) -> u64 {
        self.writes.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// How much of the file holds data, as a read sees it ([`Store::space_seen`]): it does not wait for a commit under way, which an
    /// exact count would on the serving thread.
    pub(crate) fn space(&self) -> Result<doze_store::Space, StoreError> {
        self.store()?.space_seen()
    }

    /// Gives the space deletes freed back to the disk, after the writer has finished what it was given. On a thread of the blocking
    /// pool: the region's calls wait, the rest of the server does not.
    pub(crate) async fn compact_off_loop(&mut self) -> Result<doze_store::Compacted, StoreError> {
        let store = self.store.take().ok_or(StoreError::Stopped)?;
        let (store, done) = self
            .writer
            .paused_off_loop(store, Store::compact)
            .await
            .ok_or(StoreError::Stopped)?;
        self.store = Some(store);
        done?
    }

    /// Stops the writer after it has finished what it was given, gives back the space deletes left when that is worth it, and
    /// closes the database.
    pub(crate) fn close(self) {
        self.writer.shutdown();
        if let Some(mut store) = self.store {
            // WHY: one that cannot run now runs at a later close; the data is whole either way.
            drop(store.compact_if_worth());
            // WHY: every commit was flushed when it was made, so a failure to close costs only a longer start next time.
            drop(store.close());
        }
    }
}
