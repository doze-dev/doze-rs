//! redb running on our file seam, so the database is written through the same [`Fs`] as everything else
//! and a simulated disk can cut the power under it.
//!
//! [`Fs`]: doze_fs::fs::Fs

use std::fmt;
use std::io;

use doze_fs::fs::FsFile;
use redb::StorageBackend;

/// One file, as redb wants to see it. Locking is not offered (redb then relies on the caller to open the
/// database once: a service holds a lock on its data directory for that).
pub struct FileBackend<T: FsFile>(pub T);

impl<T: FsFile> fmt::Debug for FileBackend<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileBackend").finish_non_exhaustive()
    }
}

impl<T: FsFile> StorageBackend for FileBackend<T> {
    fn len(&self) -> io::Result<u64> {
        self.0.size()
    }

    fn read(&self, offset: u64, out: &mut [u8]) -> io::Result<()> {
        let mut filled = 0;
        while filled < out.len() {
            let rest = out.get_mut(filled..).unwrap_or_default();
            match self.0.read_at(rest, offset + filled as u64)? {
                0 => return Err(io::ErrorKind::UnexpectedEof.into()),
                n => filled += n,
            }
        }
        Ok(())
    }

    fn set_len(&self, len: u64) -> io::Result<()> {
        self.0.set_len(len)
    }

    fn sync_data(&self) -> io::Result<()> {
        self.0.sync_data()
    }

    fn write(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        self.0.write_all_at(data, offset)
    }
}
