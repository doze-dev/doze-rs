//! A database and its writer, as one value a service holds.

use std::path::Path;

use doze_fs::fs::{Fs, RealFs};
use redb::{ReadTransaction, ReadableDatabase, TableDefinition};

use crate::records::Changes;
use crate::{Ack, GroupCommit, Op, Store, StoreError};

/// redb's page cache: a dev tool's records are few, and the idle heap is measured.
const CACHE_BYTES: usize = 1 << 20;

/// An open database of `&str -> bytes` tables, written through a group-commit writer.
#[derive(Debug)]
pub struct Db {
    store: Store,
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
        Ok(Self { store, writer })
    }

    /// A read of everything as it was last committed.
    ///
    /// # Errors
    /// The database refused to start a read.
    pub fn read(&self) -> Result<ReadTransaction, StoreError> {
        Ok(self.store.begin_read()?)
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

    /// Stops the writer after it has finished what it was given, and closes the database.
    pub fn close(self) {
        self.writer.shutdown();
        // WHY: every commit was flushed when it was made, so a failure to close costs only a longer start next time.
        drop(self.store.close());
    }
}
