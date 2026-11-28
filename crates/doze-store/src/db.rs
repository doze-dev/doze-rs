//! A database and its writer, as one value a service holds.

use std::path::Path;

use doze_fs::fs::{Fs, RealFs};
use redb::{ReadTransaction, ReadableDatabase, TableDefinition};

use crate::records::Changes;
use crate::rows::Rows;
use crate::{Ack, Compacted, GroupCommit, Op, Space, Store, StoreError};

/// redb's page cache: a dev tool's records are few, and the idle heap is measured.
const CACHE_BYTES: usize = 1 << 20;

/// [`Db::scan`] in a read already open: what a walk inside [`Db::read_off_loop`] calls.
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

    /// A read of everything as it was last committed.
    ///
    /// # Errors
    /// The database refused to start a read.
    pub fn read(&self) -> Result<ReadTransaction, StoreError> {
        Ok(self.store()?.begin_read()?)
    }

    /// The row under `key` of `table`, as last committed.
    ///
    /// # Errors
    /// A failed read.
    pub fn get(&self, table: &'static str, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let read = self.store()?.begin_read()?;
        let rows = read.open_table(TableDefinition::<&str, &[u8]>::new(table))?;
        Ok(rows.get(key)?.map(|v| v.value().to_vec()))
    }

    /// Calls `each` with the rows of `table` whose key is `from` or after, in key order, until it returns `false` or the table ends.
    /// A service whose table is too large to hold in memory reads it this way.
    ///
    /// # Errors
    /// A failed read.
    pub fn scan(
        &self,
        table: &'static str,
        from: &str,
        each: impl FnMut(&str, &[u8]) -> bool,
    ) -> Result<(), StoreError> {
        scan(&self.store()?.begin_read()?, table, from, each)
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
        self.store()?.read_off_loop(work).await
    }

    /// Sends rows to the writer. The [`Ack`] resolves once they are on disk; `None` when `changes` holds nothing.
    pub fn submit(&self, changes: Vec<Changes>) -> Option<Ack> {
        let changes: Vec<Changes> = changes.into_iter().filter(|c| !c.is_empty()).collect();
        if changes.is_empty() {
            return None;
        }
        let op: Op = Box::new(move |txn| {
            for c in &changes {
                c.apply(txn)?;
            }
            Ok(())
        });
        Some(self.writer.submit(op))
    }

    /// Like [`Db::submit`], with the changes to row tables ([`crate::rows::RowTable`]) of the same call in the same transaction.
    pub fn submit_with_rows(&self, changes: Vec<Changes>, rows: Vec<Rows>) -> Option<Ack> {
        let changes: Vec<Changes> = changes.into_iter().filter(|c| !c.is_empty()).collect();
        let rows: Vec<Rows> = rows.into_iter().filter(|r| !r.is_empty()).collect();
        if changes.is_empty() && rows.is_empty() {
            return None;
        }
        let op: Op = Box::new(move |txn| {
            for c in &changes {
                c.apply(txn)?;
            }
            for r in &rows {
                r.apply(txn)?;
            }
            Ok(())
        });
        Some(self.writer.submit(op))
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
