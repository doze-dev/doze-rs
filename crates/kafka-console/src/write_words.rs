//! The words of the topic-write dialogs that are made from what the broker said and what was typed: the teaching under a
//! field, a button's label, the warning of Delete records, the "in use" sentence of Delete topic. Pure functions, outside
//! the page layer, so the JSON API (`api/topic_writes.rs`) say the same thing one way.

use crate::format::number;
use crate::model::{AddView, DeleteRecordsView, InUse, ProduceView, Span, Teach};
use crate::phrases::{positions, removed};

/// The teaching under acks in the Produce dialog.
#[must_use]
pub fn acks_teach(transaction: Option<&str>) -> Teach {
    let here = match transaction {
        Some(id) => vec![
            Span::text("this is a plain write, not part of transaction "),
            Span::mono(id),
            Span::text(", so it is not held back by it."),
        ],
        None => vec![Span::text(
            "this is a plain write, not part of a transaction.",
        )],
    };
    Teach {
        says:
            "-1 answers when the batch is flushed to disk; 1 when it is written; 0 is not answered."
                .to_owned(),
        default: vec![Span::mono("-1"), Span::text(" (all).")],
        range: Vec::new(),
        takes_effect: vec![Span::text("per request.")],
        here,
    }
}

/// What the Produce dialog says of a key on this topic.
#[must_use]
pub fn key_help(v: &ProduceView) -> Vec<Span> {
    let tail = if v.compacts {
        format!(
            " keeps the latest record for every key (cleanup.policy {}), so a record with no key is refused (INVALID_RECORD). A key with no value is a deletion.",
            v.cleanup
        )
    } else {
        format!(
            " keeps its records by {}, so a record with no key is accepted. On a compacted topic it is refused. Leave Value empty for a deletion (a key with no value).",
            v.cleanup
        )
    };
    vec![Span::mono(v.topic.clone()), Span::text(tail)]
}

/// What the Add-partitions field's unit says: how many partitions the count adds, and their names.
#[must_use]
pub fn add_adds(v: &AddView) -> Option<String> {
    let want: i32 = v.count.trim().parse().ok()?;
    let more = want.checked_sub(v.have).filter(|n| *n > 0)?;
    let names: Vec<String> = (v.have..want)
        .take(3)
        .map(|p| format!("{}-{p}", v.topic))
        .collect();
    let tail = if more > 3 { ", ..." } else { "" };
    let joined = match names.as_slice() {
        [a, b] => format!("{a} and {b}"),
        _ => names.join(", "),
    };
    Some(format!("adds {more}: {joined}{tail}"))
}

/// The teaching under the Add-partitions field.
#[must_use]
pub fn add_teaching(v: &AddView) -> Teach {
    let range = match v.most {
        Some(m) => vec![
            Span::text(format!(
                "{} to {} (",
                number(i64::from(v.have) + 1),
                number(i64::from(m))
            )),
            Span::mono("--max-partitions"),
            Span::text(")."),
        ],
        None => vec![Span::text(format!(
            "{} or more.",
            number(i64::from(v.have) + 1)
        ))],
    };
    Teach {
        says: "the new total, not the number to add.".to_owned(),
        default: vec![Span::text(format!("none; it must be above {}.", v.have))],
        range,
        takes_effect: vec![Span::text(
            "at once: producers and consumers see the new partitions in their next Metadata. They start empty.",
        )],
        here: vec![Span::text(
            "records with the same key may now hash to a different partition, so order for a key is not kept across the change.",
        )],
    }
}

/// The label of the Add-partitions button.
#[must_use]
pub fn add_button(v: &AddView) -> String {
    match v
        .count
        .trim()
        .parse::<i32>()
        .ok()
        .and_then(|w| w.checked_sub(v.have))
        .filter(|n| *n > 0)
    {
        Some(1) => "Add 1 partition".to_owned(),
        Some(n) => format!("Add {n} partitions"),
        None => "Add partitions".to_owned(),
    }
}

/// The teaching under the Delete-records field.
#[must_use]
pub fn records_teaching(v: &DeleteRecordsView) -> Teach {
    let hw = number(v.high_watermark);
    Teach {
        says: "the log start moves to this offset; earlier records are no longer served."
            .to_owned(),
        default: Vec::new(),
        range: vec![Span::text(format!(
            "0 up to the high watermark ({hw}). -1 means the high watermark: delete everything."
        ))],
        takes_effect: vec![Span::text(
            "at once, kept through a restart. Below or at the start changes nothing; past the high watermark is OFFSET_OUT_OF_RANGE.",
        )],
        here: vec![Span::text(
            "it cannot be undone, and the last stable offset never falls below the new start.",
        )],
    }
}

/// The label of the Delete-records button.
#[must_use]
pub fn records_button(v: &DeleteRecordsView) -> String {
    let to: i64 = v.offset.trim().parse().unwrap_or(v.log_start);
    match removed(v.log_start, v.high_watermark, to) {
        0 => "Delete records".to_owned(),
        1 => "Delete 1 record".to_owned(),
        n => format!("Delete {} records", number(n)),
    }
}

/// The warning of the Delete-records dialog: what the offset typed removes, and who is below it.
#[must_use]
pub fn records_warning(v: &DeleteRecordsView) -> Vec<Span> {
    let to: i64 = v.offset.trim().parse().unwrap_or(v.log_start);
    let n = removed(v.log_start, v.high_watermark, to);
    let new_start = if to == -1 {
        v.high_watermark
    } else {
        to.min(v.high_watermark)
    };
    let part = format!("{}-{}", v.topic, v.partition);
    let mut out = vec![Span::strong(format!("This removes {} records", number(n)))];
    if n > 0 {
        out.push(Span::text(format!(
            ": offsets {} to {} of {part}. Offset {} stays and becomes the log start. A consumer positioned below it gets OFFSET_OUT_OF_RANGE and follows its ",
            number(v.log_start),
            number(new_start - 1),
            number(new_start)
        )));
        out.push(Span::mono("auto.offset.reset"));
        out.push(Span::text("."));
        out.extend(positions(&v.groups, new_start));
    } else {
        out.push(Span::text(format!(
            ". The log of {part} already starts at {} or later, so nothing changes.",
            number(v.log_start)
        )));
    }
    out
}

/// What the Delete-topic dialog says of who uses the topic; empty when nothing does.
#[must_use]
pub fn in_use(u: &InUse) -> Vec<Span> {
    if u.groups.is_empty() && u.transactions.is_empty() {
        return Vec::new();
    }
    let mut out = vec![Span::strong("It is in use.")];
    for (i, (g, _, members)) in u.groups.iter().take(3).enumerate() {
        out.push(Span::text(if i == 0 { " Group " } else { ", group " }));
        out.push(Span::mono(g.clone()));
        out.push(Span::text(format!(
            " has offsets committed on it ({members} {} now)",
            if *members == 1 { "member" } else { "members" }
        )));
    }
    if u.groups.len() > 3 {
        out.push(Span::text(format!(
            ", and {} more groups",
            u.groups.len() - 3
        )));
    }
    if !u.groups.is_empty() && !u.transactions.is_empty() {
        out.push(Span::text(", and"));
    }
    for (i, t) in u.transactions.iter().take(3).enumerate() {
        out.push(Span::text(if i == 0 {
            " transaction "
        } else {
            ", transaction "
        }));
        out.push(Span::mono(t.clone()));
        out.push(Span::text(" is open on it"));
    }
    out.push(Span::text(
        ". Their next requests are answered UNKNOWN_TOPIC_OR_PARTITION. Commits stay behind: if the topic is made again they will sit past its end.",
    ));
    out
}
