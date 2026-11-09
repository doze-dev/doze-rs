//! A partition's log: an ordered run of segments with exactly one active, which owns the log-end offset and
//! assigns offsets to the batches it takes.
//!
//! One writer, many readers. A [`PartitionLog`] is owned by one task and its writes take `&mut self`, so there
//! is no lock around them. Its [`LogReader`]s are cheap clones that read straight off the files up to what the
//! writer has published, so a fetch never waits for an append. Durability is a separate step: an append returns
//! a [`Ticket`], and [`PartitionLog::sync`] makes every ticket before it good with one flush.

mod append;
mod compact;
mod history;
mod open;
mod read;
mod shared;
mod skip;
mod sync;
mod times;
mod trim;

use std::path::PathBuf;
use std::sync::Arc;

use kafka_wire::ids::Offset;

pub use read::LogReader;
pub use sync::Ticket;
pub use times::OffsetAt;
pub use trim::Retention;

use crate::fs::Fs;
use crate::segment::{ActiveSegment, SealedSegment};
use shared::Shared;

/// How a partition's segments are sized.
#[derive(Debug, Clone, Copy)]
pub struct LogConfig {
    /// The active segment rolls once an append would take it past this size.
    pub segment_bytes: u64,
    /// An index entry is written about every this many bytes of log.
    pub index_interval_bytes: u64,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            segment_bytes: 128 << 20,
            index_interval_bytes: 4096,
        }
    }
}

impl<F: Fs> PartitionLog<F> {
    /// Makes `config` the one the log is judged by from the next append on: the active segment rolls at the new size,
    /// and segments made after this index at the new interval (the one being written keeps the interval it began with).
    pub fn set_config(&mut self, config: LogConfig) {
        self.config = config;
    }
}

/// Whether the previous run vouches that a partition's files were flushed (it stopped cleanly after flushing
/// them). Without it nothing read back at open is taken to be on disk until this run has flushed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Trust {
    /// The last stop was clean and covered this partition.
    Vouched,
    /// It was not, or did not.
    Unvouched,
}

/// What an append did.
#[derive(Debug)]
pub struct Appended {
    /// The offset assigned to the first record of the payload.
    pub first_offset: Offset,
    /// The log-end offset now: the offset the next record will get.
    pub next_offset: Offset,
    /// Becomes durable at the next [`PartitionLog::sync`].
    pub ticket: Ticket,
}

/// The writer of one partition's log.
pub struct PartitionLog<F: Fs> {
    fs: F,
    dir: PathBuf,
    config: LogConfig,
    sealed: Vec<SealedSegment<F>>,
    active: ActiveSegment<F>,
    /// The offset the next record gets.
    end: Offset,
    start: Offset,
    shared: Arc<Shared<F>>,
    /// Appends numbered in order, and how many of them a completed flush covers. What was read back at open
    /// without a vouching stop counts as one append nobody has flushed.
    append_seq: u64,
    synced_seq: u64,
    /// The directory's entries changed since it was last flushed.
    dir_dirty: bool,
    /// Why writes are refused, once a flush has failed.
    offline: Option<Arc<str>>,
    /// The note that this log holds, or may hold, transactional batches is on disk.
    transactional: bool,
}

impl<F: Fs> std::fmt::Debug for PartitionLog<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PartitionLog")
            .field("dir", &self.dir)
            .field("start", &self.start)
            .field("end", &self.end)
            .field("segments", &(self.sealed.len() + 1))
            .field("offline", &self.offline)
            .finish_non_exhaustive()
    }
}

impl<F: Fs> PartitionLog<F> {
    /// The oldest offset still held.
    #[must_use]
    pub fn start_offset(&self) -> Offset {
        self.start
    }

    /// The offset the next record will get. With no replication this is also the high watermark.
    #[must_use]
    pub fn end_offset(&self) -> Offset {
        self.end
    }

    /// A reader of this log: cheap to clone, and valid for as long as the log's files are.
    #[must_use]
    pub fn reader(&self) -> LogReader<F> {
        LogReader::new(Arc::clone(&self.shared))
    }

    /// Makes the segment list readers see match the writer's.
    fn publish_segments(&self) {
        let mut views: Vec<_> = self.sealed.iter().map(SealedSegment::view).collect();
        views.push(self.active.view());
        self.shared.segments.store(Arc::new(views));
    }
}

#[cfg(test)]
mod tests;
