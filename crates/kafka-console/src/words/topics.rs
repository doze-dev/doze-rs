//! The sentences of the topic pages: what a topic is, said from what the broker holds. Each is a list of spans (plain,
//! monospace, bold) so a page can set the names in the right face without ever turning data into markup.

use crate::format::{ago, bytes, count, number, span_ms};
use crate::model::{Cleanup, Span, TopicHead, TopicsView};

/// `0 and 2`, `0, 1 and 3`, `5`.
#[must_use]
pub fn list_and<T: ToString>(items: &[T]) -> String {
    let all: Vec<String> = items.iter().map(ToString::to_string).collect();
    match all.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// What Kafka said when it refused a read, in its own name and number: `TOPIC_AUTHORIZATION_FAILED (29)`.
#[must_use]
pub fn kafka_error(code: i16) -> String {
    let name = kafka_wire::ResponseError::try_from_code(code)
        .map_or_else(|| "NONE".to_owned(), |e| screaming(&e.to_string()));
    format!("{name} ({code})")
}

/// `TopicAuthorizationFailed` as Kafka writes it: `TOPIC_AUTHORIZATION_FAILED`.
fn screaming(camel: &str) -> String {
    let mut out = String::with_capacity(camel.len() + 6);
    for (i, c) in camel.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

/// The sentence for a read the broker refused.
#[must_use]
pub fn read_refused(code: i16) -> String {
    let why = match code {
        29 => " The console's principal may describe this topic but not read it.",
        1 => " The offset asked for is outside the log.",
        56 => " The partition is offline after a failed flush.",
        _ => "",
    };
    format!("The broker refused the read: {}.{why}", kafka_error(code))
}

/// A transaction's timeout the way the board writes it: `60 s` up to two minutes, then as a span.
#[must_use]
pub fn timeout(ms: i64) -> String {
    if ms > 0 && ms <= 120_000 && ms % 1000 == 0 {
        format!("{} s", ms / 1000)
    } else {
        span_ms(ms)
    }
}

/// `7 d`, or `for ever` for -1.
#[must_use]
pub fn retention(ms: i64) -> String {
    if ms < 0 {
        "for ever".to_owned()
    } else {
        span_ms(ms)
    }
}

/// How the topic treats old records, as the first clause of its sentence: `keeps records for 7 d`.
fn keeps(cleanup: Cleanup, retention_ms: Option<i64>) -> String {
    let for_a_while = || match retention_ms {
        Some(ms) if ms < 0 => "keeps records for ever".to_owned(),
        Some(ms) => format!("keeps records for {}", span_ms(ms)),
        None => "keeps records".to_owned(),
    };
    match cleanup {
        Cleanup::Delete => for_a_while(),
        Cleanup::Compact => "keeps the latest record of each key".to_owned(),
        Cleanup::Both => format!("compacts by key and {}", for_a_while()),
    }
}

/// `0.2 s ago` with the number in bold: the pieces of a sentence.
fn when(spans: &mut Vec<Span>, diff_ms: i64) {
    let text = ago(diff_ms);
    match text.strip_suffix(" ago") {
        Some(n) => {
            spans.push(Span::strong(n));
            spans.push(Span::text(" ago"));
        }
        None => spans.push(Span::text(text)),
    }
}

/// The list's sentence: `9 topics hold 21 partitions. The newest write was 0.2 s ago, to clickstream. One internal topic is hidden.`
#[must_use]
pub fn list_sentence(v: &TopicsView) -> Vec<Span> {
    let mut out = vec![
        Span::text(format!("{} hold ", count(v.total, "topic", "topics"))),
        Span::strong(number(i64::try_from(v.partitions).unwrap_or(i64::MAX))),
        Span::text(" partitions."),
    ];
    if let Some((topic, ms)) = &v.newest {
        out.push(Span::text(" The newest write was "));
        when(&mut out, v.now_ms - ms);
        out.push(Span::text(", to "));
        out.push(Span::text(topic.clone()));
        out.push(Span::text("."));
    }
    out.push(Span::text(if v.show_internal {
        " The internal topic is shown."
    } else {
        " One internal topic is hidden."
    }));
    out
}

/// `3 partitions`, `1 partition`.
fn partitions(n: usize) -> String {
    count(n, "partition", "partitions")
}

/// `orders-0 and orders-1`.
fn named(topic: &str, parts: &[i32]) -> String {
    let names: Vec<String> = parts.iter().map(|p| format!("{topic}-{p}")).collect();
    list_and(&names)
}

fn offline_clause(n: usize, down: &[i32], up: &[i32]) -> Vec<Span> {
    if up.is_empty() {
        return vec![
            Span::text(format!("{} ", partitions(n))),
            Span::text(
                "are all offline after a failed flush and refuse writes until the machine restarts.",
            ),
        ];
    }
    vec![
        Span::text(format!(
            "Partition{} {} work{}; ",
            if up.len() == 1 { "" } else { "s" },
            list_and(up),
            if up.len() == 1 { "s" } else { "" }
        )),
        Span::strong(format!(
            "partition{} {}",
            if down.len() == 1 { "" } else { "s" },
            list_and(down)
        )),
        Span::text(format!(
            " went offline after a failed flush and refuse{} writes until the machine restarts.",
            if down.len() == 1 { "s" } else { "" }
        )),
    ]
}

/// The sentence under a topic's name, in the state its data calls for.
#[must_use]
pub fn topic_sentence(h: &TopicHead) -> Vec<Span> {
    let n = h.parts.len();
    let mut out = vec![
        Span::strong(h.name.clone()),
        Span::text(format!(" has {}", partitions(n))),
    ];
    let down: Vec<i32> = h.offline().iter().map(|p| p.partition).collect();
    if !down.is_empty() {
        let up: Vec<i32> = h
            .parts
            .iter()
            .filter(|p| p.offline.is_none())
            .map(|p| p.partition)
            .collect();
        out.push(Span::text(". "));
        out.extend(offline_clause(n, &down, &up));
        return out;
    }
    out.push(Span::text(format!(
        ", {}",
        keeps(h.cleanup, h.retention_ms)
    )));
    if h.is_empty() {
        out.push(Span::text(", and is empty: offsets 0 to 0, no write yet."));
        return out;
    }
    out.push(Span::text(", holds "));
    out.push(Span::strong(number(h.records())));
    out.push(Span::text(format!(" records ({})", bytes(h.bytes()))));
    if let Some(ms) = h.last_write_ms() {
        out.push(Span::text(" and was last written "));
        out.push(Span::text(ago(h.now_ms - ms)));
    }
    out.push(Span::text("."));
    out.extend(txn_clause(h));
    out
}

/// `A transaction is open on orders-0 and orders-1: read_committed consumers are held 6 records behind on orders-0.`
fn txn_clause(h: &TopicHead) -> Vec<Span> {
    let Some(t) = h.txns.first() else {
        return Vec::new();
    };
    let on: Vec<i32> = t.partitions.iter().map(|(p, _)| *p).collect();
    let mut out = vec![Span::text(if h.txns.len() == 1 {
        " A transaction is open on "
    } else {
        " Transactions are open on "
    })];
    for (i, name) in on.iter().enumerate() {
        if i > 0 {
            out.push(Span::text(if i + 1 == on.len() { " and " } else { ", " }));
        }
        out.push(Span::strong(format!("{}-{name}", h.name)));
    }
    out.push(Span::text(": "));
    match h
        .parts
        .iter()
        .filter(|p| p.held() > 0)
        .max_by_key(|p| p.held())
    {
        Some(p) => out.push(Span::text(format!(
            "read_committed consumers are held {} behind on {}-{}.",
            count(usize::try_from(p.held()).unwrap_or(0), "record", "records"),
            h.name,
            p.partition
        ))),
        None => out.push(Span::text(
            "read_committed consumers are not held back yet.",
        )),
    }
    out
}

/// The sentence of the partitions page.
#[must_use]
pub fn partitions_sentence(h: &TopicHead) -> Vec<Span> {
    let mut out = vec![
        Span::text(format!("{}, ", partitions(h.parts.len()))),
        Span::strong(number(h.records())),
        Span::text(format!(" records, {}.", bytes(h.bytes()))),
    ];
    let moved: Vec<i32> = h
        .parts
        .iter()
        .filter(|p| p.start > 0)
        .map(|p| p.partition)
        .collect();
    if moved.is_empty() {
        out.push(Span::text(" The start is 0 on all of them."));
    } else {
        out.push(Span::text(format!(
            " The start has moved on {}: records there were deleted.",
            named(&h.name, &moved)
        )));
    }
    let held: Vec<i32> = h
        .parts
        .iter()
        .filter(|p| p.held() > 0)
        .map(|p| p.partition)
        .collect();
    if !held.is_empty() {
        out.push(Span::text(format!(
            " The last stable offset is below the high watermark on {}: an open transaction holds read_committed consumers back.",
            named(&h.name, &held)
        )));
    }
    out
}

#[cfg(test)]
mod tests;
