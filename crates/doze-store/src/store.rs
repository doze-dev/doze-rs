//! The database file: opened over the file seam, and made atomically when it is not there.

use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
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
    /// Where the file is, so its size can be read without asking the database.
    path: Arc<PathBuf>,
}

/// How much of a database file holds data, and how much is space a [`Store::compact`] would give back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Space {
    /// The file's size on disk.
    pub file: u64,
    /// The bytes in pages that hold data.
    pub used: u64,
}

impl Space {
    /// What compaction could give back: the file less the pages in use.
    #[must_use]
    pub fn reclaimable(&self) -> u64 {
        self.file.saturating_sub(self.used)
    }
}

/// What a compaction did: the file's size before and after.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Compacted {
    /// The file's size before.
    pub before: u64,
    /// The file's size after.
    pub after: u64,
}

impl Compacted {
    /// The bytes given back to the disk.
    #[must_use]
    pub fn freed(&self) -> u64 {
        self.before.saturating_sub(self.after)
    }
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
        Ok(Self {
            db: Arc::new(db),
            path: Arc::new(path),
        })
    }

    /// The database file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file's size on disk (0 when it cannot be read: a simulated disk).
    #[must_use]
    pub fn file_bytes(&self) -> u64 {
        std::fs::metadata(&*self.path).map_or(0, |m| m.len())
    }

    /// How much of the file holds data. Starts (and abandons) a write to read the allocator's count, so it waits for a commit that
    /// is under way; it changes nothing.
    ///
    /// # Errors
    /// The database refused to start a write.
    pub fn space(&self) -> Result<Space, StoreError> {
        let txn = self.db.begin_write()?;
        let stats = txn.stats()?;
        txn.abort()?;
        Ok(Space {
            file: self.file_bytes(),
            used: stats.allocated_pages() * stats.page_size() as u64,
        })
    }

    /// Moves the data to the front of the file and cuts the file after it, so space freed by deletes goes back to the disk (redb
    /// reuses a freed page but never shrinks the file by itself). The caller must hold the only handle: a writer sharing it is
    /// stopped first ([`GroupCommit::paused`](crate::GroupCommit::paused) does that).
    ///
    /// # Errors
    /// [`StoreError::Shared`] while another handle is alive; a read still open; a failed write.
    pub fn compact(&mut self) -> Result<Compacted, StoreError> {
        let before = self.file_bytes();
        let db = Arc::get_mut(&mut self.db).ok_or(StoreError::Shared)?;
        // WHY: one pass moves what it can; redb says when a further pass would move nothing.
        while db.compact()? {}
        Ok(Compacted {
            before,
            after: self.file_bytes(),
        })
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
