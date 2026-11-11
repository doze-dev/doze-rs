//! The file seam. Every operation of this crate that changes what a power cut would leave on disk goes
//! through [`Fs`]: a write, a truncation, the creation, renaming or removal of a file, a new directory, and
//! the two flushes that make any of those durable (a file's contents, a directory's entries).
//!
//! Production uses [`RealFs`]. Tests use `sim::SimFs`, a deterministic in-memory disk that can fail any
//! operation, fill up, stop the process after any operation, and rebuild what the disk would hold if the
//! power went at that moment. Nothing in this crate calls `std::fs` anywhere else.

mod durable;
mod real;

use std::io;
use std::path::Path;

pub use durable::{create_dirs_durably, read_file, write_durably};
pub use real::RealFs;

/// An entry in a directory listing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DirEntry {
    /// The name, without its directory.
    pub name: String,
    /// Whether it is a directory.
    pub is_dir: bool,
}

/// A file system.
pub trait Fs: Clone + Send + Sync + 'static {
    /// An open file.
    type File: FsFile;

    /// Opens a file that exists.
    ///
    /// # Errors
    /// `NotFound` if it does not.
    fn open_existing(&self, path: &Path) -> io::Result<Self::File>;

    /// Opens a file, creating it empty if it is not there; the flag says this call made it. A file made
    /// here has its name only in memory until its directory is flushed.
    ///
    /// # Errors
    /// Anything the system refuses, including a missing parent directory.
    fn open_or_create(&self, path: &Path) -> io::Result<(Self::File, bool)>;

    /// Renames a file, replacing any file at the new name.
    ///
    /// # Errors
    /// Anything the system refuses.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;

    /// Removes a file; one that is not there is not an error. Says whether it was there.
    ///
    /// # Errors
    /// Anything the system refuses other than a missing file.
    fn remove_file(&self, path: &Path) -> io::Result<bool>;

    /// Creates a directory whose parent exists; says whether it made it (it may already be there).
    ///
    /// # Errors
    /// Anything the system refuses, including a path where a file is.
    fn create_dir(&self, path: &Path) -> io::Result<bool>;

    /// Removes an empty directory; says whether it was there. Like a created name, the removal is only in
    /// memory until the parent directory is flushed.
    ///
    /// # Errors
    /// A directory that is not empty, or anything else the system refuses other than a missing directory.
    fn remove_dir(&self, path: &Path) -> io::Result<bool>;

    /// The entries of a directory, in no particular order.
    ///
    /// # Errors
    /// `NotFound` if there is no such directory.
    fn list_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>>;

    /// Flushes a directory's entries to stable storage. Until it has, a file created, renamed or removed in
    /// it is so only in memory, and a power cut can take a fully flushed file away with its name. Does
    /// nothing, and says so by succeeding, where the platform cannot flush a directory.
    ///
    /// # Errors
    /// A failed flush, which is a failure of the storage like any other.
    fn sync_dir(&self, path: &Path) -> io::Result<()>;

    /// Creates a directory and any missing parents; says whether it made the last one.
    ///
    /// # Errors
    /// Anything the system refuses.
    fn create_dir_all(&self, path: &Path) -> io::Result<bool> {
        let mut made = false;
        let mut so_far = std::path::PathBuf::new();
        for part in path.components() {
            so_far.push(part);
            made = self.create_dir(&so_far)?;
        }
        Ok(made)
    }
}

/// An open file. Reads and writes are positioned and take `&self`, so a reader and the writer can use one
/// handle at the same time.
pub trait FsFile: Send + Sync + 'static {
    /// Reads up to `buf.len()` bytes at `pos`; fewer only at the end of the file.
    ///
    /// # Errors
    /// Anything the system refuses.
    fn read_at(&self, buf: &mut [u8], pos: u64) -> io::Result<usize>;

    /// Writes all of `data` at `pos`, extending the file if need be.
    ///
    /// # Errors
    /// Anything the system refuses; some of the bytes may have been written.
    fn write_all_at(&self, data: &[u8], pos: u64) -> io::Result<()>;

    /// Cuts or extends the file to `len` bytes.
    ///
    /// # Errors
    /// Anything the system refuses.
    fn set_len(&self, len: u64) -> io::Result<()>;

    /// The file's size.
    ///
    /// # Errors
    /// Anything the system refuses.
    fn size(&self) -> io::Result<u64>;

    /// Flushes the file's contents to stable storage (a full device-cache flush on macOS).
    ///
    /// # Errors
    /// A failed flush. It is never retried: see `StorageError::Offline`.
    fn sync_data(&self) -> io::Result<()>;

    /// When the file was last written, in milliseconds since the Unix epoch.
    ///
    /// # Errors
    /// Anything the system refuses.
    fn modified_ms(&self) -> io::Result<i64>;
}

#[cfg(test)]
mod tests;
