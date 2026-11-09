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
pub use plan::{Facts, Params, Plan};
pub use txn::AbortedTxn;
pub use types::{CleanJob, Cleaned, LastOf, Replacement};
pub(crate) use types::{Input, temporary};
