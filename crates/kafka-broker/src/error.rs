use kafka_meta::MetaError;
use kafka_storage::StorageError;

/// Why the broker could not start, or could not do a thing to its own state.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BrokerError {
    /// The metadata database refused.
    #[error(transparent)]
    Meta(#[from] MetaError),
    /// A partition's files could not be made, read or removed.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// The data directory could not be made.
    #[error("data directory: {0}")]
    DataDir(#[from] std::io::Error),
}
