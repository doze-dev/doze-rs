//! One process per data directory.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};

/// Why the data directory could not be taken.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LockError {
    /// The directory could not be made or the lock file opened or locked.
    #[error("{what} {path}: {source}")]
    Io {
        /// What was being done.
        what: &'static str,
        /// The path it was done to.
        path: PathBuf,
        /// What the operating system said.
        source: io::Error,
    },
    /// Another process holds the directory.
    #[error("another {program} is using the data directory {dir}")]
    Held {
        /// The program that refused to share.
        program: String,
        /// The directory.
        dir: PathBuf,
    },
}

/// Holds the data directory for as long as it lives: a second process on the same directory would write
/// the same files.
#[derive(Debug)]
pub struct DataDirLock {
    _file: File,
}

impl DataDirLock {
    /// Takes the lock, making the directory if it is not there. `program` names the process in the
    /// refusal a second one gets.
    ///
    /// # Errors
    /// The directory cannot be made, or another process holds it.
    pub fn take(dir: &Path, program: &str) -> Result<Self, LockError> {
        let io = |what, path: &Path| {
            let path = path.to_path_buf();
            move |source| LockError::Io { what, path, source }
        };
        std::fs::create_dir_all(dir).map_err(io("making the data directory", dir))?;
        let path = dir.join("lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(io("opening", &path))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(LockError::Held {
                program: program.to_owned(),
                dir: dir.to_path_buf(),
            }),
            Err(TryLockError::Error(e)) => Err(io("locking", &path)(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_holder_is_refused_and_the_first_letting_go_frees_it() {
        let dir = std::env::temp_dir().join(format!("doze-lock-{}", std::process::id()));
        let first = DataDirLock::take(&dir, "doze-test").unwrap();
        let second = DataDirLock::take(&dir, "doze-test")
            .unwrap_err()
            .to_string();
        assert!(second.contains("another doze-test"), "{second}");
        drop(first);
        DataDirLock::take(&dir, "doze-test").unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
