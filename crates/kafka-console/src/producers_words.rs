//! The sentences of the Producers and transaction pages. Pure: a table of cases tests each. The wording is the boards'
//! (`topic-producers`, `txn-open`, `txn-holding`); every number is one the broker read.

use crate::format::{clock, number, span_ms};
use crate::model::{Span, TxnPart, TxnRow, TxnView};

fn t(s: impl Into<String>) -> Span {
    Span::text(s)
}

/// A transaction's timeout as the boards write it: whole seconds up to two minutes (`60 s`), else in the two largest units.
#[must_use]
pub fn timeout_text(ms: i64) -> String {
    if ms > 0 && ms % 1000 == 0 && ms <= 120_000 {
        format!("{} s", ms / 1000)
    } else {
        span_ms(ms)
    }
}

/// How long a transaction has run, and how long it has left, in milliseconds.
#[must_use]
pub fn running_ms(x: &TxnRow, now_ms: i64) -> i64 {
    (now_ms - x.since_ms).max(0)
}

/// The time left before the broker aborts a transaction; zero once it has passed.
#[must_use]
pub fn left_ms(x: &TxnRow, now_ms: i64) -> i64 {
    (x.timeout_ms - running_ms(x, now_ms)).max(0)
}

/// The percentage of the timeout that has run, for the bar.
#[must_use]
pub fn timeout_percent(x: &TxnRow, now_ms: i64) -> i64 {
    if x.timeout_ms <= 0 {
        0
    } else {
        (running_ms(x, now_ms) * 100 / x.timeout_ms).clamp(0, 100)
    }
}

fn part_list(parts: &[TxnPart], topic: &str, f: impl Fn(&TxnPart) -> Option<i64>) -> String {
    parts
        .iter()
        .filter_map(|p| f(p).map(|o| format!("{topic}-{} at {}", p.partition, number(o))))
        .collect::<Vec<_>>()
        .join(" and of ")
}

/// The sentence under the name of a transaction's page.
#[must_use]
pub fn txn_sentence(v: &TxnView) -> Vec<Span> {
    let x = &v.txn;
    let mut out = vec![
        Span::Strong(x.id.clone()),
        t(" is "),
        Span::strong(x.state.clone()),
        t(format!(
            ": producer {} epoch {}",
            x.producer_id, x.producer_epoch
        )),
    ];
    if x.ongoing() {
        out.push(t(format!(", begun at {}, ", clock(x.since_ms))));
        out.push(Span::strong(span_ms(running_ms(x, v.now_ms))));
        out.push(t(format!(
            " into its {} timeout.",
            timeout_text(x.timeout_ms)
        )));
        let held = part_list(&v.parts, &v.topic, |p| p.first);
        if !held.is_empty() {
            out.push(t(format!(" It holds the last stable offset of {held}.")));
        }
    } else {
        out.push(t(". It holds nothing back."));
    }
    out
}

/// `orders-0: 6 records, 16,098 to 16,103. orders-1: 1, at 16,050.`
#[must_use]
pub fn holds_sentence(v: &TxnView) -> Vec<Span> {
    let mut out = Vec::new();
    for (i, p) in v.parts.iter().filter(|p| p.held() > 0).enumerate() {
        let first = p.first.map_or(0, |f| f.max(p.start));
        out.push(t(format!(
            "{}{}-{}: ",
            if i == 0 { "" } else { " " },
            v.topic,
            p.partition
        )));
        out.push(Span::strong(number(p.held())));
        out.push(t(if p.held() == 1 {
            format!(", at {}.", number(first))
        } else {
            format!(" records, {} to {}.", number(first), number(p.end - 1))
        }));
    }
    if out.is_empty() {
        out.push(t("Nothing: no partition of this topic is held back."));
    }
    out
}

/// What happens if the transaction is not ended, in two parts: what comes before the refusal a fenced producer meets, and what
/// follows it. The refusal itself (a state word and Kafka's code) is drawn by the page between them.
#[must_use]
pub fn unended_parts(v: &TxnView) -> (Vec<Span>, Vec<Span>) {
    let x = &v.txn;
    if !x.ongoing() {
        return (vec![t("It is over: nothing is left to abort.")], Vec::new());
    }
    let left = left_ms(x, v.now_ms);
    let at = x.since_ms + x.timeout_ms;
    let markers = v
        .parts
        .iter()
        .map(|p| format!("{}-{} (at {})", v.topic, p.partition, number(p.end)))
        .collect::<Vec<_>>()
        .join(" and ");
    let mut out = if left == 0 {
        vec![t(
            "The timeout has passed: the broker aborts it at its next look",
        )]
    } else {
        vec![
            t("At "),
            Span::strong(clock(at)),
            t(format!(
                ", {} from now, the broker aborts it",
                span_ms(left)
            )),
        ]
    };
    out.push(t(format!(
        ", whatever the producer is doing. It writes an abort marker to {markers}, so the last stable offset moves up to the high watermark, and it fences producer {}: the epoch goes from {} to {}, and a Produce at epoch {} is ",
        x.producer_id,
        x.producer_epoch,
        i32::from(x.producer_epoch) + 1,
        x.producer_epoch,
    )));
    let after = vec![t(format!(
        ". A well-behaved client asks InitProducerId again and starts a new transaction; read_committed consumers skip the {} records of the aborted one.",
        number(v.held())
    ))];
    (out, after)
}

/// What happens if the transaction is not ended, as one sentence.
#[must_use]
pub fn unended_sentence(v: &TxnView) -> Vec<Span> {
    let (mut before, after) = unended_parts(v);
    if !after.is_empty() {
        before.push(t("refused with INVALID_PRODUCER_EPOCH (47)"));
        before.extend(after);
    }
    before
}

/// The sentence under the name of the page that says who is held.
#[must_use]
pub fn holding_sentence(v: &TxnView) -> Vec<Span> {
    let Some(p) = v.parts.iter().find(|p| p.partition == v.tail_partition) else {
        return vec![t("No partition of this topic is held back.")];
    };
    vec![
        t("The last stable offset is the first offset of the oldest open transaction. On "),
        Span::text(format!("{}-{} that is ", v.topic, p.partition)),
        Span::strong(number(p.last_stable)),
        t(format!(
            ", {} below the high watermark ",
            number_word(p.end - p.last_stable)
        )),
        Span::strong(number(p.end)),
        t("."),
    ]
}

fn number_word(n: i64) -> String {
    match n {
        1 => "one".to_owned(),
        2 => "two".to_owned(),
        3 => "three".to_owned(),
        4 => "four".to_owned(),
        5 => "five".to_owned(),
        6 => "six".to_owned(),
        7 => "seven".to_owned(),
        8 => "eight".to_owned(),
        9 => "nine".to_owned(),
        n => number(n),
    }
}

#[cfg(test)]
mod tests;
