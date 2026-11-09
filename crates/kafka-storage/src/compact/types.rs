//! What a round of cleaning is given and what it leaves.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use kafka_wire::ids::Offset;

use super::plan::{Params, Plan};
use super::txn::AbortedTxn;
use crate::fs::Fs;
use crate::segment::{SegmentView, index_path, log_path};

/// A sealed segment given to a job.
pub(crate) struct Input<F: Fs> {
    pub(crate) base: Offset,
    /// The offset after its last batch.
    pub(crate) next: Offset,
    pub(crate) view: SegmentView<F>,
}

/// What a producer has in the log that must not be lost: the offset its last data batch ends at, if it has data, and
/// the epoch of its last marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastOf {
    pub data_last_offset: Option<i64>,
    pub marker_epoch: i16,
}

/// A round of cleaning, ready to run.
pub struct CleanJob<F: Fs> {
    pub(crate) fs: F,
    pub(crate) dir: PathBuf,
    /// Every sealed segment, oldest first.
    pub(crate) segments: Vec<Input<F>>,
    pub(crate) plan: Plan,
    pub(crate) params: Params,
    pub(crate) index_interval: u64,
    pub(crate) aborted: Vec<AbortedTxn>,
    pub(crate) producers: HashMap<i64, LastOf>,
}

/// A segment's new files, written beside the old ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    pub(crate) base: Offset,
    /// The size of the segment this was made from: the writer puts it in place only if that is what it still has.
    pub(crate) from_size: u64,
}

/// What a round did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cleaned {
    /// The segments to put in place.
    pub replacements: Vec<Replacement>,
    /// Cleaning has got this far: the first offset not cleaned.
    pub clean_to: i64,
    /// Records looked at, and records left.
    pub records_read: u64,
    pub records_kept: u64,
}

/// The temporary files of a segment's replacement.
pub(crate) fn temporary(dir: &Path, base: Offset) -> (PathBuf, PathBuf) {
    let tmp = |mut p: PathBuf| {
        p.as_mut_os_string().push(".tmp");
        p
    };
    (tmp(log_path(dir, base)), tmp(index_path(dir, base)))
}

impl<F: Fs> std::fmt::Debug for CleanJob<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CleanJob")
            .field("dir", &self.dir)
            .field("plan", &self.plan)
            .field("segments", &self.segments.len())
            .finish_non_exhaustive()
    }
}
