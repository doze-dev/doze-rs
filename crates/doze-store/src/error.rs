//! What can go wrong with a store.

use std::io;

/// A failure of the store.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The database refused: a failed write or flush, or a file it cannot make sense of.
    #[error("database: {0}")]
    Database(redb::Error),
    /// The file system refused while the database was being made.
    #[error("database file: {0}")]
    Io(#[from] io::Error),
    /// A record that this version cannot read.
    #[error("record is unreadable: {0}")]
    Corrupt(String),
    /// A commit failed, so the writer refuses everything until the process starts again (a flush that
    /// failed once may report success later without having written anything).
    #[error("the writer stopped after a failed commit: {0}")]
    Failed(String),
    /// The writer has been shut down.
    #[error("the writer has been shut down")]
    Stopped,
}

/// Every error type redb has becomes the one variant, through redb's own unifying `Error`.
macro_rules! from_redb {
    ($($t:ty),* $(,)?) => {$(
        impl From<$t> for StoreError {
            fn from(e: $t) -> Self {
                Self::Database(redb::Error::from(e))
            }
        }
    )*};
}

from_redb!(
    redb::DatabaseError,
    redb::TransactionError,
    redb::TableError,
    redb::StorageError,
    redb::CommitError,
    redb::SetDurabilityError,
);
