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

impl Cleaned {
    /// The round in parts of at most `most` segments each, to be put in place one after the other: putting a segment in
    /// place takes the writer three directory flushes, and a log of hundreds of segments must not hold its writer for all of
    /// them at once. Only the last part says how far cleaning got, so that the checkpoint moves only when every segment is
    /// in place.
    #[must_use]
    pub fn in_parts(self, most: usize) -> Vec<Cleaned> {
        let most = most.max(1);
        let mut parts: Vec<Cleaned> = self
            .replacements
            .chunks(most)
            .map(|replacements| Cleaned {
                replacements: replacements.to_vec(),
                ..Cleaned::default()
            })
            .collect();
        if parts.is_empty() {
            parts.push(Cleaned::default());
        }
        if let Some(last) = parts.last_mut() {
            last.clean_to = self.clean_to;
            last.records_read = self.records_read;
            last.records_kept = self.records_kept;
        }
        parts
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn round(n: i64) -> Cleaned {
        Cleaned {
            replacements: (0..n)
                .map(|i| Replacement {
                    base: Offset::new(i * 10).unwrap(),
                    from_size: 1,
                })
                .collect(),
            clean_to: 77,
            records_read: 5,
            records_kept: 3,
        }
    }

    #[test]
    fn a_round_is_put_in_place_in_parts_and_only_the_last_says_how_far_it_got() {
        let parts = round(7).in_parts(3);
        let sizes: Vec<_> = parts.iter().map(|p| p.replacements.len()).collect();
        assert_eq!(sizes, [3, 3, 1]);
        let to: Vec<_> = parts.iter().map(|p| p.clean_to).collect();
        assert_eq!(
            to,
            [0, 0, 77],
            "the checkpoint moves when every part is in place"
        );
        assert_eq!(parts[2].records_read + parts[0].records_read, 5);
        // Every segment is in exactly one part, in order.
        let bases: Vec<_> = parts
            .iter()
            .flat_map(|p| p.replacements.iter().map(|r| r.base.get()))
            .collect();
        assert_eq!(bases, [0, 10, 20, 30, 40, 50, 60]);
    }

    #[test]
    fn a_round_with_nothing_to_replace_is_still_one_part_that_moves_the_checkpoint() {
        let parts = round(0).in_parts(3);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].clean_to, 77);
        assert_eq!(round(2).in_parts(0).len(), 2, "never zero at a time");
    }
}
