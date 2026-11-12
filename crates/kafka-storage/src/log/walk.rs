//! A bounded, read-only walk of a partition's batches for the console: what each segment holds, which offsets compaction
//! removed, and which deletions a round has kept.
//!
//! Nothing is kept and nothing is written. The walk reads batch headers (61 bytes each) and, only for a batch that
//! compaction thinned or that carries a delete horizon, the batch itself, within a budget; a log bigger than the budget is
//! reported as walked in part, never guessed.

use kafka_wire::batch::{Batch, HEADER_LEN, Marker, Peek};

use super::LogReader;
use crate::compact::Facts;
use crate::fs::{Fs, FsFile};
use crate::segment::SegmentView;

/// How many marker offsets a segment keeps (the newest).
const MOST_MARKERS: usize = 256;

/// What a walk may spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// The most batch headers read in all.
    pub batches: usize,
    /// The most bytes of whole batches read (thinned ones, and ones with a delete horizon).
    pub batch_bytes: u64,
    /// The most removed-offset ranges kept in all.
    pub gaps: usize,
    /// The most deletions listed.
    pub tombstones: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            batches: 200_000,
            batch_bytes: 32 << 20,
            gaps: 20_000,
            tombstones: 200,
        }
    }
}

/// One segment as the walk found it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SegmentWalk {
    /// Its first offset (the name of its files).
    pub base: i64,
    /// The offset after its last: where the next segment begins, or the log ends.
    pub next: i64,
    /// The bytes of its log file readers may see.
    pub bytes: u64,
    /// When its log file was last written, where the system says.
    pub modified_ms: Option<i64>,
    /// Every batch of it was read (not cut short by the budget).
    pub walked: bool,
    /// How many batches and how many records it holds.
    pub batches: u32,
    pub records: u64,
    /// The timestamp of its first batch (its newest record's, as the header says), and the newest of any batch.
    pub oldest_ms: i64,
    pub newest_ms: Option<i64>,
    /// The offsets it no longer has, as (first, how many) ranges in offset order. Offsets never move, so a batch a
    /// round dropped and a record a round took out of a batch both leave one.
    pub gaps: Vec<(i64, i64)>,
    /// Records removed that sit in a batch the walk did not open (over the budget): counted, not placed.
    pub unplaced: u64,
    /// The transaction markers in it, the newest 256.
    pub markers: Vec<MarkerNote>,
}

impl SegmentWalk {
    /// The offsets the segment spans.
    #[must_use]
    pub fn span(&self) -> i64 {
        (self.next - self.base).max(0)
    }

    /// The offsets in its gaps.
    #[must_use]
    pub fn removed(&self) -> i64 {
        self.gaps.iter().map(|g| g.1).sum::<i64>() + i64::try_from(self.unplaced).unwrap_or(0)
    }
}

/// A commit or abort marker the log holds: where it is, when it was written, and what it says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerNote {
    pub offset: i64,
    pub at_ms: i64,
    pub commit: bool,
}

/// A deletion a round has kept, with the time after which the next round removes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tombstone {
    pub offset: i64,
    /// Its key, the first 200 bytes of it.
    pub key: Vec<u8>,
    /// The record's own timestamp: when the deletion was written.
    pub written_ms: i64,
    /// The delete horizon its batch was given: the next round after this removes it.
    pub goes_after_ms: i64,
}

/// What a walk found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Walk {
    /// The log start and end now.
    pub start: i64,
    pub end: i64,
    /// Every segment, oldest first.
    pub segments: Vec<SegmentWalk>,
    /// The deletions kept, by the time they go.
    pub tombstones: Vec<Tombstone>,
    /// Some batch or range was left out for want of budget.
    pub partial: bool,
}

impl Walk {
    /// The sealed segments as the cleaner's planner is given them, and the active segment's base: the same facts
    /// `PartitionLog::clean_job` reads, so the planner judges a console's copy as it judges the log. A segment with no
    /// record timestamp counts as written when its file was last written, as the cleaner does.
    #[must_use]
    pub fn planner_facts(&self) -> (Vec<Facts>, i64) {
        let Some((active, sealed)) = self.segments.split_last() else {
            return (Vec::new(), self.end);
        };
        let facts = sealed
            .iter()
            .map(|s| Facts {
                base: s.base,
                next: s.next,
                size: s.bytes,
                newest_ms: s.newest_ms.or(s.modified_ms).unwrap_or(0),
                oldest_ms: s.oldest_ms,
            })
            .collect();
        (facts, active.base)
    }
}

/// What the walk has left to spend.
struct Spent {
    left: Budget,
    partial: bool,
}

impl<F: Fs> LogReader<F> {
    /// Surveys the log's segments within `budget`. A copy of what the files hold; the log is not touched.
    #[must_use]
    pub fn survey(&self, budget: Budget) -> Walk {
        let views = self.shared.segments.load();
        let end = self.end_offset().get();
        let mut spent = Spent {
            left: budget,
            partial: false,
        };
        let mut out = Walk {
            start: self.start_offset().get(),
            end,
            ..Walk::default()
        };
        for (i, view) in views.iter().enumerate() {
            let next = views.get(i + 1).map_or(end, |n| n.base().get());
            let seg = one(view, next, &mut spent, &mut out.tombstones);
            out.segments.push(seg);
        }
        out.tombstones.sort_by_key(|t| (t.goes_after_ms, t.offset));
        out.tombstones.truncate(budget.tombstones);
        out.partial = spent.partial || out.segments.iter().any(|s| !s.walked);
        out
    }
}

fn one<F: Fs>(
    view: &SegmentView<F>,
    next: i64,
    spent: &mut Spent,
    tombs: &mut Vec<Tombstone>,
) -> SegmentWalk {
    let committed = view.committed();
    let mut seg = SegmentWalk {
        base: view.base().get(),
        next,
        bytes: committed,
        modified_ms: view.log_file().modified_ms().ok(),
        walked: true,
        ..SegmentWalk::default()
    };
    let (mut pos, mut expect) = (0u64, seg.base);
    let mut head = [0u8; HEADER_LEN];
    while pos < committed {
        if spent.left.batches == 0 {
            seg.walked = false;
            spent.partial = true;
            return seg;
        }
        spent.left.batches -= 1;
        let read = view.read_into(pos, &mut head).unwrap_or(0);
        let Some(peek) = head.get(..read).and_then(|h| Peek::read(h).ok()) else {
            seg.walked = false;
            return seg;
        };
        let after = pos.saturating_add(peek.total_len);
        if after > committed {
            seg.walked = false;
            return seg;
        }
        note_batch(&mut seg, &peek, expect, spent);
        if peek.attributes.is_control() {
            note_marker(&mut seg, view, pos, &peek);
        }
        let batch_next = peek.next_offset().unwrap_or(expect);
        if wants_batch(&peek) {
            open_batch(view, pos, &peek, (&mut seg, spent), tombs);
        }
        expect = batch_next.max(expect);
        pos = after;
    }
    // The offsets the segment spans after its last batch are gone too (a round dropped the batches that held them).
    if expect < next {
        push_gap(&mut seg, spent, expect, next - expect);
    }
    seg
}

/// Counts a batch and the offsets between it and the one before.
fn note_batch(seg: &mut SegmentWalk, peek: &Peek, expect: i64, spent: &mut Spent) {
    seg.batches += 1;
    seg.records += u64::from(peek.record_count);
    if seg.batches == 1 {
        seg.oldest_ms = peek.max_timestamp.get();
    }
    let t = peek.max_timestamp.get();
    seg.newest_ms = Some(seg.newest_ms.map_or(t, |n| n.max(t)));
    if peek.base_offset > expect {
        push_gap(seg, spent, expect, peek.base_offset - expect);
    }
}

fn push_gap(seg: &mut SegmentWalk, spent: &mut Spent, first: i64, count: i64) {
    if count <= 0 {
        return;
    }
    if let Some(last) = seg.gaps.last_mut().filter(|g| g.0 + g.1 == first) {
        last.1 += count;
    } else if spent.left.gaps > 0 {
        spent.left.gaps -= 1;
        seg.gaps.push((first, count));
    } else {
        spent.partial = true;
        seg.unplaced += u64::try_from(count).unwrap_or(0);
    }
}

/// Notes a marker: its batch is read (it is a few dozen bytes) to see whether it commits or aborts. One that cannot be read is
/// not listed.
fn note_marker<F: Fs>(seg: &mut SegmentWalk, view: &SegmentView<F>, pos: u64, peek: &Peek) {
    let kind = usize::try_from(peek.total_len)
        .ok()
        .and_then(|n| view.read(pos, n).ok())
        .and_then(|bytes| Marker::read(&bytes));
    if let Some(kind) = kind {
        seg.markers.push(MarkerNote {
            offset: peek.base_offset,
            at_ms: peek.max_timestamp.get(),
            commit: kind == Marker::Commit,
        });
        if seg.markers.len() > MOST_MARKERS {
            seg.markers.remove(0);
        }
    }
}

/// Whether the batch is worth reading whole: compaction took records out of it, or it carries a delete horizon.
fn wants_batch(peek: &Peek) -> bool {
    let span = u64::from(peek.last_offset_delta) + 1;
    u64::from(peek.record_count) < span
        || (peek.attributes.has_delete_horizon() && !peek.attributes.is_control())
}

/// Reads the batch whole, to place the offsets its thinned records left and to list the deletions it holds.
fn open_batch<F: Fs>(
    view: &SegmentView<F>,
    pos: u64,
    peek: &Peek,
    (seg, spent): (&mut SegmentWalk, &mut Spent),
    tombs: &mut Vec<Tombstone>,
) {
    let span = i64::from(peek.last_offset_delta) + 1;
    let missing = span - i64::from(peek.record_count);
    let unaffordable = spent.left.batch_bytes < peek.total_len;
    let batch = if unaffordable {
        None
    } else {
        spent.left.batch_bytes -= peek.total_len;
        usize::try_from(peek.total_len)
            .ok()
            .and_then(|n| view.read(pos, n).ok())
            .and_then(|b| Batch::parse_stored(b).ok())
    };
    let Some(batch) = batch else {
        spent.partial = true;
        seg.unplaced += u64::try_from(missing.max(0)).unwrap_or(0);
        return;
    };
    let (base, horizon) = (peek.base_offset, batch.first_timestamp().get());
    let keep_tombs = peek.attributes.has_delete_horizon() && !peek.attributes.is_control();
    let mut at = base;
    let mut holes = Vec::new();
    let read = batch.each_record_full(8 << 20, |r| {
        if r.offset > at {
            holes.push((at, r.offset - at));
        }
        at = r.offset + 1;
        if keep_tombs && r.value.is_none() && tombs.len() < spent.left.tombstones * 4 {
            tombs.push(Tombstone {
                offset: r.offset,
                key: r
                    .key
                    .unwrap_or_default()
                    .iter()
                    .take(200)
                    .copied()
                    .collect(),
                written_ms: r.timestamp,
                goes_after_ms: horizon,
            });
        }
        true
    });
    if read.is_err() {
        spent.partial = true;
        seg.unplaced += u64::try_from(missing.max(0)).unwrap_or(0);
        return;
    }
    let batch_next = base + span;
    if at < batch_next {
        holes.push((at, batch_next - at));
    }
    for (first, count) in holes {
        push_gap(seg, spent, first, count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spent(gaps: usize) -> Spent {
        Spent {
            left: Budget {
                gaps,
                ..Budget::default()
            },
            partial: false,
        }
    }

    #[test]
    fn gaps_that_touch_are_one_range_and_gaps_apart_are_two() {
        let (mut seg, mut s) = (SegmentWalk::default(), spent(10));
        push_gap(&mut seg, &mut s, 0, 2);
        push_gap(&mut seg, &mut s, 2, 3);
        push_gap(&mut seg, &mut s, 10, 1);
        push_gap(&mut seg, &mut s, 11, 0);
        assert_eq!(seg.gaps, [(0, 5), (10, 1)]);
        assert!(!s.partial);
    }

    #[test]
    fn a_gap_past_the_budget_is_counted_and_says_the_walk_is_partial() {
        let (mut seg, mut s) = (SegmentWalk::default(), spent(1));
        push_gap(&mut seg, &mut s, 0, 2);
        push_gap(&mut seg, &mut s, 10, 4);
        assert_eq!(
            (seg.gaps.as_slice(), seg.unplaced, s.partial),
            ([(0, 2)].as_slice(), 4, true)
        );
        assert_eq!(seg.removed(), 6);
    }
}
