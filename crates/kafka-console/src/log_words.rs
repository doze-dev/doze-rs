//! The sentences of the Log page, built from a [`LogView`]. Pure: a table of cases tests each. The wording is the boards'
//! (`topic-log`, `log-planner`, `log-retention`, `log-empty`); every number in a sentence is one the broker read.

mod planner;
mod retention;

use crate::format::span_ms;
use crate::model::{LogCompaction, LogView};

pub use planner::{Rule, RuleTone, round_sentence, rules, still_to_come, would_rewrite};
pub use retention::{Expiry, expiry, retention_rules};

/// `61 %`: a share as a whole per cent.
#[must_use]
pub fn pct(share: f64) -> String {
    format!("{} %", (share * 100.0).round() as i64)
}

/// A ratio as the topic's config says it: `0.5`, `0.7`.
#[must_use]
pub fn ratio_text(r: f64) -> String {
    format!("{r}")
}

/// How long the cleaner has until its next look, from when it last looked; `None` when it has not looked yet.
#[must_use]
pub fn next_look_ms(c: &LogCompaction, now_ms: i64) -> Option<i64> {
    c.looked_at_ms.map(|at| (at + c.every_ms - now_ms).max(0))
}

/// `15 s`, `5 m`, `2 h`: an interval as the boards write it in a sentence; anything that is not a whole unit says it in two.
#[must_use]
pub fn every(ms: i64) -> String {
    match ms {
        m if m > 0 && m % 3_600_000 == 0 => format!("{} h", m / 3_600_000),
        m if m > 0 && m % 60_000 == 0 && m < 3_600_000 => format!("{} m", m / 60_000),
        m if m > 0 && m % 1000 == 0 && m < 60_000 => format!("{} s", m / 1000),
        m => span_ms(m),
    }
}

/// `15s`, `5m`, `2h`: the same interval as a flag takes it.
#[must_use]
pub fn flag(ms: i64) -> String {
    every(ms).replace(' ', "")
}

/// The number of the segment at `index` as the page calls it (the first is 1).
#[must_use]
pub fn number_of(index: usize) -> usize {
    index + 1
}

/// The segments the next round would rewrite: the sealed ones from the one that holds the first dirty offset up to where
/// cleaning stops.
#[must_use]
pub fn dirty_segments(v: &LogView, c: &LogCompaction) -> Vec<usize> {
    v.segments
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.active && s.base >= c.planner.dirty_start && s.base < c.planner.upto)
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests;
