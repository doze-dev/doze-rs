//! A database and its writer, as one value a service holds.

use std::path::Path;
use std::sync::Arc;

use doze_fs::fs::{Fs, RealFs};
use redb::{ReadTransaction, ReadableDatabase, TableDefinition};

use crate::pending::{Change, Overlaid, Pending, Span, View};
use crate::records::Changes;
use crate::rows::Rows;
use crate::{Ack, Compacted, GroupCommit, Op, Space, Store, StoreError};

/// redb's page cache: a dev tool's records are few, and the idle heap is measured.
const CACHE_BYTES: usize = 1 << 20;

/// Past every key of a text table: no UTF-8 holds the byte 0xFF.
const PAST_TEXT: &[u8] = &[0xFF];

/// One submit's rows: applied by the writer, and laid over the file for a read until then ([`Pending`]).
struct Batch {
    changes: Vec<Changes>,
    rows: Vec<Rows>,
}

impl Overlaid for Batch {
    fn changes(&self, table: &str, f: &mut dyn FnMut(Change<'_>)) {
        for c in &self.changes {
            c.seen(table, f);
        }
        for r in &self.rows {
            r.seen(table, f);
        }
    }
}

/// [`Db::scan`] in a read already open: what a walk inside [`Db::read_off_loop`] calls. It sees what was committed when the read
/// began, without the queued writes ([`Db::read_off_loop`] waits for them first).
///
/// # Errors
/// A failed read.
pub fn scan(
    read: &ReadTransaction,
    table: &'static str,
    from: &str,
    mut each: impl FnMut(&str, &[u8]) -> bool,
) -> Result<(), StoreError> {
    let rows = read.open_table(TableDefinition::<&str, &[u8]>::new(table))?;
    for row in rows.range::<&str>(from..)? {
        let (key, bytes) = row?;
        if !each(key.value(), bytes.value()) {
            break;
        }
    }
    Ok(())
}

/// An open database of `&str -> bytes` tables, written through a group-commit writer.
#[derive(Debug)]
pub struct Db {
    /// `None` only while [`Db::compact_off_loop`] has it on a blocking thread (never seen by a caller, who holds `&mut` then), or
    /// after the runtime stopped during one.
    store: Option<Store>,
    writer: GroupCommit,
}

impl Db {
    /// Opens (or makes) the database `file` in `dir`, with `tables` existing from the first commit.
    ///
    /// # Errors
    /// A file system or database failure.
    pub fn open(dir: &Path, file: &str, tables: &[&'static str]) -> Result<Self, StoreError> {
        let fs = RealFs;
        fs.create_dir_all(dir)?;
        let store = Store::open(&fs, dir, file, CACHE_BYTES, |txn| {
            for name in tables {
                txn.open_table(TableDefinition::<&str, &[u8]>::new(name))?;
            }
            Ok(())
        })?;
        let writer = GroupCommit::start(store.clone())?;
        Ok(Self {
            store: Some(store),
            writer,
        })
    }

    fn store(&self) -> Result<&Store, StoreError> {
        self.store.as_ref().ok_or(StoreError::Stopped)
    }

    /// A read of everything as it was last committed. It does not see the queued writes ([`Db::pending`]): a service that does not
    /// wait for its commits reads through [`Db::get`], [`Db::scan`] or a row table instead.
    ///
    /// # Errors
    /// The database refused to start a read.
    pub fn read(&self) -> Result<ReadTransaction, StoreError> {
        Ok(self.store()?.begin_read()?)
    }

    /// The writes submitted and not yet committed, which [`Db::get`], [`Db::scan`] and the row tables lay over the file.
    #[must_use]
    pub fn pending(&self) -> &Pending {
        self.writer.pending()
    }

    /// The queued writes, taken before a read begins (the order matters: see [`crate::pending`]).
    pub(crate) fn view(&self) -> View {
        self.writer.pending().view()
    }

    /// The row under `key` of `table`: as last committed, or as a write queued since left it.
    ///
    /// # Errors
    /// A failed read.
    pub fn get(&self, table: &'static str, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        if let Some(queued) = self.view().get(table, key.as_bytes()) {
            return Ok(queued);
        }
        let read = self.store()?.begin_read()?;
        let rows = read.open_table(TableDefinition::<&str, &[u8]>::new(table))?;
        Ok(rows.get(key)?.map(|v| v.value().to_vec()))
    }

    /// Calls `each` with the rows of `table` whose key is `from` or after, in key order, until it returns `false` or the table ends.
    /// A service whose table is too large to hold in memory reads it this way. The queued writes are laid over the file.
    ///
    /// # Errors
    /// A failed read.
    pub fn scan(
        &self,
        table: &'static str,
        from: &str,
        mut each: impl FnMut(&str, &[u8]) -> bool,
    ) -> Result<(), StoreError> {
        let view = self.view();
        let read = self.store()?.begin_read()?;
        if view.is_empty() {
            return scan(&read, table, from, each);
        }
        let rows = read.open_table(TableDefinition::<&str, &[u8]>::new(table))?;
        view.merge(
            Span {
                table,
                lo: from.as_bytes(),
                hi: PAST_TEXT,
                reverse: false,
            },
            rows.range::<&str>(from..)?,
            |(k, v)| (k.value().as_bytes(), v.value()),
            // WHY: every key here is text: the file's, and the queued ones made from `&str`.
            |k, v| each(std::str::from_utf8(k).unwrap_or_default(), v),
        )?;
        Ok(())
    }

    /// Runs `work` with a read of everything as last committed, on a thread of the blocking pool, and answers what it returns. For
    /// a read too long for the serving thread (a walk of every row of a large table): the caller awaits it, every other task runs
    /// meanwhile. One job for the whole read: the pool is small, so a caller must not split a walk into many.
    ///
    /// # Errors
    /// A failed read, what `work` returns, or [`StoreError::Stopped`] when the runtime stopped before the job could run. A panic in
    /// `work` is the caller's.
    pub async fn read_off_loop<T: Send + 'static>(
        &self,
        work: impl FnOnce(&ReadTransaction) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError> {
        // The read cannot see the queued writes, so it waits for them to be committed first.
        self.writer.pending().settled().await;
        self.store()?.read_off_loop(work).await
    }

    /// Sends rows to the writer. The [`Ack`] resolves once they are on disk; until then [`Db::get`], [`Db::scan`] and the row tables
    /// see them ([`crate::pending`]). `None` when `changes` holds nothing.
    pub fn submit(&self, changes: Vec<Changes>) -> Option<Ack> {
        self.submit_with_rows(changes, Vec::new())
    }

    /// Queues an operation a read cannot see before its commit (for a test that holds the writer).
    #[doc(hidden)]
    pub fn submit_op(&self, op: Op) -> Ack {
        self.writer.submit(op)
    }

    /// Like [`Db::submit`], with the changes to row tables ([`crate::rows::RowTable`]) of the same call in the same transaction.
    pub fn submit_with_rows(&self, changes: Vec<Changes>, rows: Vec<Rows>) -> Option<Ack> {
        let changes: Vec<Changes> = changes.into_iter().filter(|c| !c.is_empty()).collect();
        let rows: Vec<Rows> = rows.into_iter().filter(|r| !r.is_empty()).collect();
        if changes.is_empty() && rows.is_empty() {
            return None;
        }
        let batch = Arc::new(Batch { changes, rows });
        let applied = Arc::clone(&batch);
        let op: Op = Box::new(move |txn| {
            for c in &applied.changes {
                c.apply(txn)?;
            }
            for r in &applied.rows {
                r.apply(txn)?;
            }
            Ok(())
        });
        Some(self.writer.submit_seen(op, batch))
    }

    /// How much of the file holds data, and how much a [`Db::compact`] would give back, as a read sees it ([`Store::space_seen`]): it
    /// does not wait for a commit under way, so an actor on the serving thread may ask it.
    ///
    /// # Errors
    /// A failed read.
    pub fn space(&self) -> Result<Space, StoreError> {
        self.store()?.space_seen()
    }

    /// Gives the space that deletes freed back to the disk. Waits for queued writes first. Blocks for as long as that takes: an
    /// actor on the serving thread calls [`Db::compact_off_loop`].
    ///
    /// # Errors
    /// A read still open, a failed write, or a writer that could not be started again.
    pub fn compact(&mut self) -> Result<Compacted, StoreError> {
        let store = self.store.as_mut().ok_or(StoreError::Stopped)?;
        self.writer.paused(store, Store::compact)?
    }

    /// [`Db::compact`] on a thread of the blocking pool: the wait for queued writes and the compaction (seconds on a large file)
    /// leave the serving thread free; the caller awaits them.
    ///
    /// # Errors
    /// As [`Db::compact`], or [`StoreError::Stopped`] when the runtime stopped first.
    pub async fn compact_off_loop(&mut self) -> Result<Compacted, StoreError> {
        let store = self.store.take().ok_or(StoreError::Stopped)?;
        let (store, done) = self
            .writer
            .paused_off_loop(store, Store::compact)
            .await
            .ok_or(StoreError::Stopped)?;
        self.store = Some(store);
        done?
    }

    /// Stops the writer after it has finished what it was given, gives back the space deletes left when that is worth it
    /// ([`Store::compact_if_worth`]), and closes the database.
    pub fn close(self) {
        self.writer.shutdown();
        if let Some(mut store) = self.store {
            // WHY: a compaction that cannot run now (a read still open) runs at a later close; the data is whole either way.
            drop(store.compact_if_worth());
            // WHY: every commit was flushed when it was made, so a failure to close costs only a longer start next time.
            drop(store.close());
        }
    }
}
