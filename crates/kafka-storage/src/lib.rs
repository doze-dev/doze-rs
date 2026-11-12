//! Partition logs on disk: segments, index, recovery, retention, compaction.
//!
//! Everything here is synchronous and runtime-free: a log is a value with `&mut self` methods, and every
//! file operation goes through [`fs::Fs`], which is what lets the whole crate run on a simulated disk.

pub mod compact;
pub mod error;
mod index;
pub mod log;
mod logstart;
pub mod marker;
mod retention;
mod scan;
mod segment;
pub mod topic_dirs;
mod transactional;

// The file seam moved to `doze-fs` (A1); these keep every `kafka_storage::fs` path working.
pub use doze_fs::fs;
#[cfg(any(test, feature = "sim"))]
pub use doze_fs::sim;

pub use error::StorageError;
pub use log::{
    Appended, Budget, LogConfig, LogReader, PartitionLog, Retention, SegmentInfo, SegmentWalk,
    Ticket, Tombstone, Trust, Walk, segments_on_disk,
};
pub use retention::{count_to_drop, enforce_closed};

#[cfg(test)]
mod testutil;
