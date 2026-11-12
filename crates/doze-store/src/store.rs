//! The database file: opened over the file seam, and made atomically when it is not there.

use std::io;
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;

use doze_fs::fs::Fs;
use redb::{Database, Durability, WriteTransaction};

use crate::{FileBackend, StoreError};

/// A handle to one redb database. Cloning it shares the database; the last handle to be closed closes it.
///
/// It dereferences to the [`Database`], so reads (`begin_read`) are redb's own.
#[derive(Debug, Clone)]
pub struct Store {
    db: Arc<Database>,
}

impl Deref for Store {
    type Target = Database;

    fn deref(&self) -> &Database {
        &self.db
    }
}

impl Store {
    /// Opens the database `file` in `dir`, making it if the directory has none. `init` runs once, in the
    /// transaction that makes the database, and opens the tables so they exist from the first commit.
    /// `cache_bytes` is redb's page cache.
    ///
    /// The database is made under another name and moved into place once it is whole and flushed, so a
    /// crash while it is being made leaves no file that redb would have to make sense of.
    ///
    /// # Errors
    /// A file system or database failure.
    pub fn open<F: Fs>(
        fs: &F,
        dir: &Path,
        file: &str,
        cache_bytes: usize,
        init: impl FnOnce(&WriteTransaction) -> Result<(), StoreError>,
    ) -> Result<Self, StoreError> {
        let path = dir.join(file);
        let opened = match fs.open_existing(&path) {
            Ok(opened) => opened,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                make(fs, dir, file, cache_bytes, init)?;
                fs.open_existing(&path)?
            }
            Err(e) => return Err(e.into()),
        };
        let db = builder(cache_bytes).create_with_backend(FileBackend(opened))?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Starts a write whose commit flushes, whatever the library's default says.
    ///
    /// # Errors
    /// The database refused to start a write.
    pub fn begin(&self) -> Result<WriteTransaction, StoreError> {
        let mut txn = self.db.begin_write()?;
        txn.set_durability(Durability::Immediate)?;
        Ok(txn)
    }

    /// Closes the database, flushing it once more, if this is the last handle to it. (Dropping closes
    /// it too, but cannot say if that failed; the commits themselves were flushed when they were made,
    /// so a failure here costs only a longer start next time.)
    ///
    /// # Errors
    /// A failed flush.
    pub fn close(self) -> Result<(), StoreError> {
        match Arc::into_inner(self.db) {
            Some(db) => Ok(db.close()?),
            None => Ok(()),
        }
    }
}

fn builder(cache_bytes: usize) -> redb::Builder {
    let mut b = Database::builder();
    b.set_cache_size(cache_bytes);
    b
}

/// Makes a database with its tables and puts it at `dir/file`.
fn make<F: Fs>(
    fs: &F,
    dir: &Path,
    file: &str,
    cache_bytes: usize,
    init: impl FnOnce(&WriteTransaction) -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    let staged = dir.join(format!("{file}.tmp"));
    // Whatever an earlier attempt left is not trusted.
    fs.remove_file(&staged)?;
    let (opened, _) = fs.open_or_create_private(&staged)?;
    let db = builder(cache_bytes).create_with_backend(FileBackend(opened))?;
    let txn = db.begin_write()?;
    init(&txn)?;
    txn.commit()?;
    // Closed by name, not by dropping: closing flushes once more, and a flush that fails must stop the
    // file being put in place (a dropped database would swallow it, and the file's later flushes may
    // report success without writing anything).
    db.close()?;
    fs.rename(&staged, &dir.join(file))?;
    fs.sync_dir(dir)?;
    Ok(())
}
