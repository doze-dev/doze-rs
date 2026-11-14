//! The sentences of the topic write pages that are built from numbers: when a record was acknowledged, what a topic
//! holds, what a transaction holds back. Pure functions (tables of cases in `tests.rs`).

use crate::format::{civil, number};
use crate::model::Span;

/// `22:14:41.208`, the time of day in UTC with milliseconds.
#[must_use]
pub fn clock_ms(ms: i64) -> String {
    let c = civil(ms);
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        c.hour,
        c.minute,
        c.second,
        ms.rem_euclid(1000)
    )
}

/// When the broker answers a produce, by `acks`.
#[must_use]
pub fn acked(acks: i16) -> &'static str {
    match acks {
        -1 => "acknowledged after the flush (acks -1)",
        1 => "acknowledged once written (acks 1)",
        0 => "not acknowledged: acks 0 does not wait for an answer",
        _ => "acknowledged",
    }
}

/// How many records deleting before `to` removes from a log that runs from `start` to `high`: -1 is the high watermark; a
/// target below the start removes nothing and one past the high watermark removes everything there is (the broker
/// refuses it).
#[must_use]
pub fn removed(start: i64, high: i64, to: i64) -> i64 {
    let to = if to == -1 { high } else { to.min(high) };
    (to - start).max(0)
}

/// What the groups that have committed on the partition would see if the log started at `to`: each one named with where it
/// is, and whether it is below the new start.
#[must_use]
pub fn positions(groups: &[(String, i64)], to: i64) -> Vec<Span> {
    let mut out = Vec::new();
    for (g, at) in groups.iter().take(3) {
        out.push(Span::text(" "));
        out.push(Span::mono(g.clone()));
        let below = *at < to;
        out.push(Span::text(format!(
            " is at {}: {}.",
            number(*at),
            if below {
                "its next fetch is out of range"
            } else {
                "it is not affected"
            }
        )));
    }
    if groups.len() > 3 {
        out.push(Span::text(format!(
            " {} more groups have committed here.",
            groups.len() - 3
        )));
    }
    out
}

#[cfg(test)]
mod tests;
