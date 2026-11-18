//! Compaction: keeping the latest record for each key (source: Apache Kafka's log cleaner). A pure planner says when a
//! log is due; a job learns the latest offset of every key in the dirty part and writes new files for the segments that
//! change, leaving the log as it is; the writer puts them in place one segment at a time, safe against a stop between
//! any two steps (see `log/compact.rs`).

mod checkpoint;
mod job;
mod offset_map;
pub mod plan;
mod retain;
mod rewrite;
mod source;
mod txn;
mod types;

pub(crate) use checkpoint::{read as read_checkpoint, write as write_checkpoint};

/// Where cleaning last got to in the partition whose files are in `dir`: the first offset not yet cleaned (0 if no round
/// has put its work in place). Read-only; for the console.
#[must_use]
pub fn checkpoint_of<F: crate::fs::Fs>(fs: &F, dir: &std::path::Path) -> i64 {
    read_checkpoint(fs, dir).get()
}
pub use plan::{Facts, Limit, Params, Plan, Reading, judge, reading};
pub use txn::AbortedTxn;
pub use types::{CleanJob, Cleaned, LastOf, Replacement};
pub(crate) use types::{Input, temporary};
