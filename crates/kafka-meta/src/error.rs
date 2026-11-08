//! What can go wrong with the metadata.

use std::io;

use kafka_wire::ids::TopicName;

use crate::names::Collision;

/// A failure of the metadata store.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MetaError {
    /// The database refused: a failed write or flush, or a file it cannot make sense of. After a failed
    /// commit the database refuses everything until the process starts again, as a partition does.
    #[error("metadata database: {0}")]
    Database(redb::Error),
    /// The file system refused while the database was being made.
    #[error("metadata file: {0}")]
    Io(#[from] io::Error),
    /// A record that this version cannot read.
    #[error("metadata record is unreadable: {0}")]
    Corrupt(String),
    /// A topic of that name exists.
    #[error("topic '{0}' already exists")]
    TopicExists(TopicName),
    /// A topic whose name would share a directory or a metric name with one that exists.
    #[error("{0}")]
    Collides(Collision),
}

/// Every error type redb has becomes the one variant, through redb's own unifying `Error`.
macro_rules! from_redb {
    ($($t:ty),* $(,)?) => {$(
        impl From<$t> for MetaError {
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
