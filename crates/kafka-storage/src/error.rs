//! The one error kind that crosses the crate boundary.

use std::io;

/// Why a storage operation failed. The broker maps all of these to `KAFKA_STORAGE_ERROR` except
/// [`StorageError::OutOfRange`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StorageError {
    /// A file operation failed.
    #[error("storage failed while {doing}: {source}")]
    Io {
        /// What was being done.
        doing: &'static str,
        /// The system's error.
        #[source]
        source: io::Error,
    },
    /// A flush of this partition failed once. The kernel may have dropped the unflushed pages and will report
    /// success next time, so nothing more is acknowledged until a restart decides from what is really on disk.
    #[error(
        "an earlier flush of this partition failed ({cause}); its writes are refused until the broker restarts"
    )]
    Offline {
        /// The first failure.
        cause: String,
    },
    /// An offset outside what the log holds.
    #[error("offset {requested} is out of range [{start}, {end}]")]
    OutOfRange {
        /// What was asked for.
        requested: i64,
        /// The log-start offset.
        start: i64,
        /// The log-end offset.
        end: i64,
    },
}

/// Adds what was being done to a failed file operation.
pub(crate) trait Doing<T> {
    fn doing(self, what: &'static str) -> Result<T, StorageError>;
}

impl<T> Doing<T> for io::Result<T> {
    fn doing(self, what: &'static str) -> Result<T, StorageError> {
        self.map_err(|source| StorageError::Io {
            doing: what,
            source,
        })
    }
}
