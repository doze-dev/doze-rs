//! What a logs page reads from its [`LogIndex`]: the rows in view, the counts, the hits.

use super::logindex::{EVENT, HIT, LogIndex, NUMBER};
use super::logs::{self, Line};
use super::time::Zone;
use crate::ring::LogRing;
use std::ops::Range;
use std::time::SystemTime;

impl LogIndex {
    /// How many lines are shown.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Each level's count, over every line of the page (shown or not).
    pub fn counts(&self) -> [usize; 4] {
        self.counts
    }

    /// How many lines the filter matched, over every level.
    pub fn matches(&self) -> usize {
        self.matches
    }

    /// The run numbers seen, oldest first.
    pub fn runs(&self) -> Vec<u32> {
        self.runs.iter().copied().collect()
    }

    /// The instances with lines, in the order they first appear: each gets the same colour every time.
    pub fn instances(&self) -> &[String] {
        &self.instances
    }

    /// Which of the six instance colours `inst` is drawn in.
    pub fn hue(&self, inst: &str) -> usize {
        self.instances.iter().position(|i| i == inst).unwrap_or(0) % 6
    }

    /// The rows `range` of the list, as drawn.
    pub fn rows(
        &self,
        ring: &LogRing,
        range: Range<usize>,
        now: SystemTime,
        zone: Zone,
    ) -> Vec<Line> {
        let merged = self.query.merged();
        let n = self.len();
        self.entries
            .range(range.start.min(n)..range.end.min(n))
            .map(|&e| {
                let i = e & NUMBER;
                if e & EVENT != 0 {
                    usize::try_from(i)
                        .ok()
                        .and_then(|i| self.events.get(i))
                        .map(|d| logs::event_line(d, now, zone))
                        .unwrap_or_default()
                } else {
                    ring.at(i)
                        .map(|l| logs::line_of(&l, merged, now, zone))
                        .unwrap_or_default()
                }
            })
            .collect()
    }

    /// Whether row `row` is a hit (find mode).
    pub fn is_hit(&self, row: usize) -> bool {
        self.entries.get(row).is_some_and(|&e| e & HIT != 0)
    }

    /// How many hits there are.
    pub fn hit_count(&self) -> usize {
        self.hit_n
    }

    /// The hit after row `from` (`back`: before it), wrapping; and its place among the hits, from 0.
    pub fn next_hit(&self, from: Option<usize>, back: bool) -> Option<(usize, usize)> {
        let hits: Vec<usize> = (0..self.len()).filter(|&r| self.is_hit(r)).collect();
        let n = hits.len();
        if n == 0 {
            return None;
        }
        let at = match (from, back) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(f), false) => hits.iter().position(|&h| h > f).unwrap_or(0),
            (Some(f), true) => hits.iter().rposition(|&h| h < f).unwrap_or(n - 1),
        };
        hits.get(at).map(|&row| (row, at))
    }

    /// A row's place, stable while lines slide out of the ring (to hold a selection or a scroll still).
    pub fn anchor(&self, row: usize) -> u64 {
        self.dropped + row as u64
    }

    /// The row at an anchor, while it is still shown.
    pub fn row_of(&self, anchor: u64) -> Option<usize> {
        usize::try_from(anchor.checked_sub(self.dropped)?)
            .ok()
            .filter(|&r| r < self.len())
    }
}
