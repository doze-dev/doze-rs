//! One broker per data directory.

use std::fs::{File, OpenOptions};
use std::path::Path;

use anyhow::{Context, Result, bail};

/// Holds the data directory for as long as it lives: a second broker on the same directory would write the same logs
/// and the same database.
#[derive(Debug)]
pub(crate) struct DataDirLock {
    _file: File,
}

impl DataDirLock {
    /// Takes the lock, making the directory if it is not there.
    ///
    /// # Errors
    /// The directory cannot be made, or another process holds it.
    pub(crate) fn take(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("making the data directory {}", dir.display()))?;
        let path = dir.join("lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) => {
                bail!(
                    "another doze-kafka is using the data directory {}",
                    dir.display()
                )
            }
            Err(std::fs::TryLockError::Error(e)) => {
                Err(e).with_context(|| format!("locking {}", path.display()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_holder_is_refused_and_the_first_letting_go_frees_it() {
        let dir = std::env::temp_dir().join(format!("doze-lock-{}", std::process::id()));
        let first = DataDirLock::take(&dir).unwrap();
        let second = DataDirLock::take(&dir).unwrap_err().to_string();
        assert!(second.contains("another doze-kafka"), "{second}");
        drop(first);
        DataDirLock::take(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
