//! The sentences of the topic write pages that are built from numbers: when a record was acknowledged, what a topic
//! holds, what a transaction holds back. Pure functions (tables of cases in `tests.rs`).

use crate::format::{civil, number, span_ms};
use crate::model::Span;
use crate::model::write::TopicHead;

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

/// `orders-0`, `orders-0 and orders-1`, `orders-0, orders-1 and orders-2`.
fn partitions_of(topic: &str, mut parts: Vec<i32>) -> String {
    parts.sort_unstable();
    parts.dedup();
    let names: Vec<String> = parts.iter().map(|p| format!("{topic}-{p}")).collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// The sentence under a topic's name: its partitions, how long it keeps records, how many it holds, and the
/// transaction that holds read_committed consumers back.
#[must_use]
pub fn head_sentence(h: &TopicHead) -> Vec<Span> {
    let mut out = vec![
        Span::strong(h.name.clone()),
        Span::text(format!(" has {} partitions", h.partitions)),
    ];
    match h.retention_ms {
        Some(ms) if ms >= 0 && h.cleanup != "compact" => {
            out.push(Span::text(format!(", keeps records for {}", span_ms(ms))));
        }
        Some(-1) => out.push(Span::text(", keeps records for ever")),
        _ => {}
    }
    out.push(Span::text(", holds "));
    out.push(Span::strong(number(h.records)));
    out.push(Span::text(" records and was last written just now."));
    if !h.transactions.is_empty() {
        let parts: Vec<i32> = h.transactions.iter().flat_map(|(_, p)| p.clone()).collect();
        out.push(Span::text(" A transaction is open on "));
        out.push(Span::strong(partitions_of(&h.name, parts)));
        if let Some((p, n)) = h.held_back.first() {
            out.push(Span::text(format!(
                ": read_committed consumers are held {n} records behind on {}.",
                format_args!("{}-{p}", h.name)
            )));
        } else {
            out.push(Span::text("."));
        }
    }
    out
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
