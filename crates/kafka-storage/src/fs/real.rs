//! The real file system: `std::fs` with positioned reads and writes.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

use super::{DirEntry, Fs, FsFile};

/// The operating system's files.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealFs;

impl Fs for RealFs {
    type File = RealFile;

    fn open_existing(&self, path: &Path) -> io::Result<RealFile> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Ok(RealFile(file))
    }

    fn open_or_create(&self, path: &Path) -> io::Result<(RealFile, bool)> {
        // `create_new` is atomic: whichever call gets AlreadyExists knows it did not make the file.
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(file) => Ok((RealFile(file), true)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                Ok((self.open_existing(path)?, false))
            }
            Err(e) => Err(e),
        }
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<bool> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn create_dir(&self, path: &Path) -> io::Result<bool> {
        match std::fs::create_dir(path) {
            Ok(()) => Ok(true),
            // Made by someone else in the meantime is fine; a file in its place is not.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            // A name that is not UTF-8 is not one of ours.
            if let Ok(name) = entry.file_name().into_string() {
                out.push(DirEntry {
                    name,
                    is_dir: entry.file_type()?.is_dir(),
                });
            }
        }
        Ok(out)
    }

    #[cfg(unix)]
    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        File::open(path)?.sync_all()
    }

    #[cfg(not(unix))]
    fn sync_dir(&self, _path: &Path) -> io::Result<()> {
        // Windows cannot flush a directory handle; its renames and creates are flushed with the file.
        Ok(())
    }
}

/// An open file of the operating system.
#[derive(Debug)]
pub struct RealFile(File);

impl FsFile for RealFile {
    #[cfg(unix)]
    fn read_at(&self, buf: &mut [u8], pos: u64) -> io::Result<usize> {
        std::os::unix::fs::FileExt::read_at(&self.0, buf, pos)
    }

    #[cfg(windows)]
    fn read_at(&self, buf: &mut [u8], pos: u64) -> io::Result<usize> {
        std::os::windows::fs::FileExt::seek_read(&self.0, buf, pos)
    }

    #[cfg(unix)]
    fn write_all_at(&self, data: &[u8], pos: u64) -> io::Result<()> {
        std::os::unix::fs::FileExt::write_all_at(&self.0, data, pos)
    }

    #[cfg(windows)]
    fn write_all_at(&self, data: &[u8], pos: u64) -> io::Result<()> {
        let (mut data, mut pos) = (data, pos);
        while !data.is_empty() {
            let n = std::os::windows::fs::FileExt::seek_write(&self.0, data, pos)?;
            data = data.get(n..).unwrap_or_default();
            pos += n as u64;
        }
        Ok(())
    }

    fn set_len(&self, len: u64) -> io::Result<()> {
        self.0.set_len(len)
    }

    fn size(&self) -> io::Result<u64> {
        Ok(self.0.metadata()?.len())
    }

    fn sync_data(&self) -> io::Result<()> {
        self.0.sync_data()
    }

    fn modified_ms(&self) -> io::Result<i64> {
        let since = self.0.metadata()?.modified()?.duration_since(UNIX_EPOCH);
        Ok(since.map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX)))
    }
}
