//! Partition logs on disk: segments, index, recovery, retention, compaction.
//!
//! Everything here is synchronous and runtime-free: a log is a value with `&mut self` methods, and every
//! file operation goes through [`fs::Fs`], which is what lets the whole crate run on a simulated disk.

pub mod compact;
pub mod error;
pub mod fs;
mod index;
pub mod log;
mod logstart;
pub mod marker;
mod retention;
mod scan;
mod segment;
#[cfg(any(test, feature = "sim"))]
pub mod sim;
pub mod topic_dirs;
mod transactional;

pub use error::StorageError;
pub use log::{Appended, LogConfig, LogReader, PartitionLog, Retention, Ticket, Trust};
pub use retention::enforce_closed;

#[cfg(test)]
mod testutil;
