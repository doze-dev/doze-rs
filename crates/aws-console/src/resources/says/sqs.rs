//! SQS: how a queue's page speaks.

use super::super::page::{list, plural};
use super::super::path;
use super::super::{Page, Read, Verb};
use crate::model::{Badge, Needs, ResourceRow, Tone};

/// The title of the list of queues that use this one as their dead-letter queue.
const SOURCES: &str = "Queues that use it as their dead-letter queue";

/// The queues that use this one as their dead-letter queue (the names in its list), when there are any.
fn sources(r: &Read<'_>) -> Vec<String> {
    r.sub(SOURCES)
        .map(|s| {
            s.rows
                .iter()
                .filter_map(|row| row.cells.first().cloned().flatten())
                .collect()
        })
        .unwrap_or_default()
}

fn is_dead_letter(r: &Read<'_>) -> bool {
    !sources(r).is_empty()
}

/// `pick-lists has 2 messages waiting and 1 in flight. A message received more than 5 times moves to pick-lists-dlq.`
fn sentence(r: &Read<'_>) -> Option<String> {
    let waiting = r.number("Attributes.ApproximateNumberOfMessages")?;
    let in_flight = r
        .number("Attributes.ApproximateNumberOfMessagesNotVisible")
        .unwrap_or(0);
    let delayed = r
        .number("Attributes.ApproximateNumberOfMessagesDelayed")
        .unwrap_or(0);
    let name = r.name;
    let from = sources(r);
    let mut out = if from.is_empty() {
        format!(
            "{name} has {} waiting and {in_flight} in flight",
            plural(waiting, "message", "messages")
        )
    } else {
        format!(
            "{name} holds {}{}",
            plural(waiting, "message", "messages"),
            if in_flight > 0 {
                format!(", {in_flight} in flight")
            } else {
                String::new()
            }
        )
    };
    if delayed > 0 {
        out.push_str(&format!(", {delayed} delayed"));
    }
    out.push('.');
    if !from.is_empty() {
        out.push_str(&format!(" It is the dead-letter queue of {}.", list(&from)));
    }
    // SQS keeps its redrive policy as a JSON text; a path through it reads it.
    let target = r
        .text("Attributes.RedrivePolicy.deadLetterTargetArn")
        .map(|arn| path::tail(&arn));
    if let (Some(target), Some(n)) = (target, r.number("Attributes.RedrivePolicy.maxReceiveCount"))
    {
        out.push_str(&format!(
            " A message received more than {n} times moves to {target}."
        ));
    }
    Some(out)
}

fn badges(r: &Read<'_>) -> Vec<Badge> {
    let mut out = vec![Badge::new(
        if r.flag("Attributes.FifoQueue") {
            "FIFO"
        } else {
            "Standard"
        },
        Tone::Neutral,
    )];
    if is_dead_letter(r) {
        out.push(Badge::new("Dead-letter queue", Tone::Info));
    }
    out
}

/// A queue's header: what it is doing, what it is, and what can be done to it.
pub const QUEUE: Page = Page {
    sentence: Some(sentence),
    badges: Some(badges),
    verbs: &[
        Verb::ui("edit", "Edit"),
        Verb::ui("delete", "Delete").danger(),
        Verb::call("purge", "Purge", "PurgeQueue", Needs::Confirm).danger(),
        Verb::ui("messages", "Send and receive messages"),
        Verb::ui("redrive", "Start DLQ redrive").when(is_dead_letter),
    ],
    summary: Some(summary),
};

/// A number in a cell of a row.
fn cell(row: &ResourceRow, at: usize) -> i64 {
    row.cells
        .get(at)
        .cloned()
        .flatten()
        .and_then(|c| c.parse().ok())
        .unwrap_or(0)
}

/// `7 queues hold 58 messages waiting and 6 in flight. pick-lists-dlq is the only dead-letter queue with a message in it.`
fn summary(rows: &[ResourceRow]) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    let waiting: i64 = rows.iter().map(|r| cell(r, 1)).sum();
    let in_flight: i64 = rows.iter().map(|r| cell(r, 2)).sum();
    let dead: std::collections::BTreeSet<String> = rows
        .iter()
        .filter_map(|r| r.cells.get(3).cloned().flatten())
        .collect();
    let loaded: Vec<String> = rows
        .iter()
        .filter(|r| dead.contains(&r.name) && cell(r, 1) > 0)
        .map(|r| r.name.clone())
        .collect();
    let n = i64::try_from(rows.len()).unwrap_or(i64::MAX);
    let mut out = format!(
        "{} {} {} waiting",
        plural(n, "queue", "queues"),
        if n == 1 { "holds" } else { "hold" },
        plural(waiting, "message", "messages")
    );
    if in_flight > 0 {
        out.push_str(&format!(" and {in_flight} in flight"));
    }
    out.push('.');
    match loaded.as_slice() {
        [] => {}
        [one] => out.push_str(&format!(
            " {one} is the only dead-letter queue with a message in it."
        )),
        many => out.push_str(&format!(
            " {} are dead-letter queues with messages in them.",
            list(many)
        )),
    }
    Some(out)
}
