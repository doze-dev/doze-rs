//! The sentences of the Log page, built from a [`LogView`]. Pure: a table of cases tests each. The wording is the boards'
//! (`topic-log`, `log-planner`, `log-retention`, `log-empty`); every number in a sentence is one the broker read.

mod planner;
mod retention;

use crate::format::{bytes, count, number, span_ms};
use crate::model::{LogCleaner, LogCompaction, LogSegment, LogState, LogView, Span};
use crate::ui::button::State;

pub use planner::{Rule, RuleTone, round_sentence, rules, still_to_come, would_rewrite};
pub use retention::{Expiry, expiry, retention_rules, retention_sentence};

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

/// What the title row says of the log: a word and its colour.
#[must_use]
pub fn state_word(v: &LogView) -> (State, &'static str) {
    match v.state() {
        LogState::Empty => (State::Idle, "Empty"),
        LogState::Due => (State::Ok, "Compacted"),
        LogState::NotDue => (State::Idle, "Not due"),
        LogState::Retention => (State::Ok, "Delete"),
    }
}

/// How many records or offsets the log holds, as the sentence says it: records when none were removed, offsets else.
fn holds(v: &LogView) -> (String, &'static str) {
    let offsets = (v.end - v.start).max(0);
    if v.segments.iter().all(|s| s.removed() == 0) {
        (number(offsets), "records")
    } else {
        (number(offsets), "offsets")
    }
}

fn segments_and_size(v: &LogView) -> String {
    format!(
        "{}, {}",
        count(v.segments.len(), "segment", "segments"),
        bytes(v.bytes())
    )
}

/// The sentence under the name.
#[must_use]
pub fn head_sentence(v: &LogView) -> Vec<Span> {
    let (n, unit) = holds(v);
    match (v.state(), &v.compaction, &v.retention) {
        (LogState::Empty, _, _) if v.end > 0 => vec![Span::text(format!(
            "No records left. Everything before offset {} was deleted, by retention or by Delete records; the log grows from there.",
            number(v.end)
        ))],
        (LogState::Empty, _, _) => vec![Span::text(
            "No records yet. A new partition starts with one empty active segment at offset 0; the log grows from there.",
        )],
        (_, Some(c), _) => compaction_sentence(v, c, (&n, unit)),
        (_, None, Some(_)) => retention_sentence(v),
        _ => vec![
            Span::strong(n),
            Span::text(format!(" {unit} in {}.", segments_and_size(v))),
        ],
    }
}

fn compaction_sentence(v: &LogView, c: &LogCompaction, (n, unit): (&str, &str)) -> Vec<Span> {
    let p = &c.planner;
    let ratio = ratio_text(c.rules.min_dirty_ratio);
    let mut out = vec![
        Span::strong(n.to_owned()),
        Span::text(format!(" {unit} in {}. ", segments_and_size(v))),
    ];
    let next = next_look_ms(c, v.now_ms);
    let look = |out: &mut Vec<Span>| match next {
        Some(ms) => {
            out.push(Span::text(
                ", so the cleaner will take it at its next look in ",
            ));
            out.push(Span::strong(span_ms(ms.max(0))));
            out.push(Span::text("."));
        }
        None => out.push(Span::text(
            ", so the cleaner will take it at its next look.",
        )),
    };
    match &c.cleaner {
        LogCleaner::Disabled => out.push(Span::text(
            "The cleaner is switched off (--log-cleaner-enable), so nothing will clean this log.",
        )),
        LogCleaner::Running { .. } => out.push(Span::text("A round is running on it now.")),
        _ if p.due
            && p.waited_too_long
            && p.ratio().is_none_or(|r| r <= c.rules.min_dirty_ratio) =>
        {
            out.push(Span::text(
                "Its oldest dirty record has waited past max.compaction.lag.ms",
            ));
            look(&mut out);
        }
        _ if p.due => {
            let share = p.ratio().map_or_else(String::new, pct);
            out.push(Span::strong(share));
            out.push(Span::text(format!(
                " of the cleanable log is dirty, more than the {} it waits for",
                pct(c.rules.min_dirty_ratio)
            )));
            look(&mut out);
        }
        _ => not_due(&mut out, c, &ratio, v),
    }
    out
}

fn not_due(out: &mut Vec<Span>, c: &LogCompaction, ratio: &str, v: &LogView) {
    use crate::model::LogLimit;
    let p = &c.planner;
    let looking = format!(
        "it is looking at this log every {} and leaving it alone.",
        every(c.every_ms)
    );
    match (p.ratio(), p.limit) {
        (Some(r), _) => {
            out.push(Span::strong(pct(r)));
            out.push(Span::text(format!(
                " of the log is dirty and the cleaner waits for more than {}, so {looking}",
                pct(c.rules.min_dirty_ratio)
            )));
        }
        (None, LogLimit::Transaction) => out.push(Span::text(format!(
            "An open transaction holds cleaning back at offset {}, so {looking}",
            number(p.upto)
        ))),
        (None, LogLimit::TooNew) => out.push(Span::text(format!(
            "Every dirty segment is newer than min.compaction.lag.ms ({}), so {looking}",
            span_ms(c.rules.min_lag_ms)
        ))),
        (None, LogLimit::Active) => out.push(Span::text(format!(
            "Nothing sealed is dirty ({} is the active segment), so {looking} (min.cleanable.dirty.ratio {ratio}.)",
            segment_name(v.segments.last())
        ))),
    }
}

fn segment_name(s: Option<&LogSegment>) -> String {
    s.map_or_else(
        || "the log".to_owned(),
        |s| format!("the segment from {}", number(s.base)),
    )
}

#[cfg(test)]
mod tests;
