//! The words of the Data page (docs/port/data-control.md): each kind of thing on disk, what a topic keeps, the heads-up when
//! the disk needs someone, what Empty keeps, and what each action did. Rust decides them so the page, Home and the top bar say
//! them one way.

use kafka_broker::{DataUsage, Emptied, EmptiedAll, Erased, Keeps};

use crate::format::{bytes, count, number};
use crate::model::{AlarmTone, Cleanup, DataKind, DataKindRow, DiskAlarm};

/// Below this much room the disk is nearly full, whatever its size.
pub const LOW_FREE_BYTES: u64 = 2 << 30;
/// Below this share of the volume (in thousandths) the disk is nearly full.
pub const LOW_FREE_PERMILLE: u64 = 50;
/// The data directory is worth a look once it is this share of the volume (in thousandths) ...
pub const LARGE_SHARE_PERMILLE: u64 = 100;
/// ... and at least this big.
pub const LARGE_BYTES: u64 = 1 << 30;
/// A topic that keeps everything is worth a look once it is this big.
pub const UNBOUNDED_BYTES: u64 = 1 << 30;

/// The phrase typed to confirm Empty all topics.
pub const EMPTY_ALL_PHRASE: &str = "empty all topics";
/// The phrase typed to confirm Erase everything.
pub const ERASE_PHRASE: &str = "erase everything";

fn permille(part: u64, whole: u64) -> u64 {
    if whole == 0 {
        0
    } else {
        u64::try_from(u128::from(part) * 1000 / u128::from(whole)).unwrap_or(1000)
    }
}

/// A share in thousandths, for the page: `u32` (at most 1000).
#[must_use]
pub fn share(part: u64, whole: u64) -> u32 {
    u32::try_from(permille(part, whole).min(1000)).unwrap_or(1000)
}

/// What the policy keeps, short, for the table: `7 days`, `1.0 GiB per partition`, `latest per key`, `none`.
#[must_use]
pub fn retention_short(k: &Keeps) -> String {
    if k.compact && k.max_age_ms.is_none() && k.max_bytes.is_none() {
        return "latest per key".to_owned();
    }
    let time = k.max_age_ms.map(kafka_broker::duration_words);
    let size = k.max_bytes.map(|b| format!("{} per partition", bytes(b)));
    match (time, size) {
        (Some(t), Some(s)) => format!("{t} or {s}"),
        (Some(t), None) => t,
        (None, Some(s)) => s,
        (None, None) => "none".to_owned(),
    }
}

/// The topic's `cleanup.policy`, as the topics list names it.
#[must_use]
pub fn cleanup(k: &Keeps) -> Cleanup {
    match (k.compact, k.delete) {
        (true, true) => Cleanup::Both,
        (true, false) => Cleanup::Compact,
        _ => Cleanup::Delete,
    }
}

/// Each kind of thing in the data directory, largest first; a kind with nothing in it is left out (the topics are always
/// there).
#[must_use]
pub fn kinds(u: &DataUsage, subjects: Option<usize>) -> Vec<DataKindRow> {
    let partitions: i64 = u.topics.iter().map(|t| i64::from(t.partitions)).sum();
    let mut held = vec![
        format!(
            "{} groups' committed offsets",
            number(i64::try_from(u.groups).unwrap_or(i64::MAX))
        ),
        count(u.transactional_ids, "transactional id", "transactional ids"),
        "ACLs and users".to_owned(),
    ];
    if let Some(n) = subjects {
        held.push(count(n, "schema subject", "schema subjects"));
    }
    let mut rows = vec![
        DataKindRow {
            kind: DataKind::Topics,
            label: "Topics".to_owned(),
            bytes: u.topics_bytes(),
            note: format!(
                "{} with {} partitions: their segments and indexes.",
                count(u.topics.len(), "topic", "topics"),
                number(partitions)
            ),
        },
        DataKindRow {
            kind: DataKind::Metadata,
            label: "Metadata store".to_owned(),
            bytes: u.meta_bytes,
            note: format!("meta.redb: the topics, {}.", held.join(", ")),
        },
    ];
    if !u.strays.is_empty() {
        rows.push(DataKindRow {
            kind: DataKind::Strays,
            label: "Belonging to no topic".to_owned(),
            bytes: u.stray_bytes(),
            note: format!(
                "{} left by a metadata store that was lost or replaced. Erase everything removes them; so does a topic made with the same name.",
                count(u.strays.len(), "partition directory", "partition directories")
            ),
        });
    }
    if u.other_bytes > 0 || u.other_files > 0 {
        rows.push(DataKindRow {
            kind: DataKind::Other,
            label: "Other files".to_owned(),
            bytes: u.other_bytes,
            note: format!(
                "{}: the lock, the record of the last clean stop, anything else put here.",
                count(
                    usize::try_from(u.other_files).unwrap_or(usize::MAX),
                    "file",
                    "files"
                )
            ),
        });
    }
    rows.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.kind.cmp(&b.kind)));
    rows
}

/// The heads-up, if the disk needs someone: nearly full first, then a data directory that takes a large share of it, then a
/// large topic that keeps everything.
#[must_use]
pub fn alarm(u: &DataUsage) -> Option<DiskAlarm> {
    let total = u.total();
    if let Some(space) = u.space
        && (space.free < LOW_FREE_BYTES || permille(space.free, space.total) < LOW_FREE_PERMILLE)
    {
        return Some(DiskAlarm {
            tone: AlarmTone::Bad,
            headline: "The disk is nearly full".to_owned(),
            detail: format!(
                "{} free of {}, and doze-kafka's data is {} of it. Once the disk is full, writes to it fail: empty or delete what you no longer need.",
                bytes(space.free),
                bytes(space.total),
                bytes(total)
            ),
            topic: None,
        });
    }
    if let Some(space) = u.space
        && total >= LARGE_BYTES
        && permille(total, space.total) >= LARGE_SHARE_PERMILLE
    {
        let pct = permille(total, space.total) / 10;
        return Some(DiskAlarm {
            tone: AlarmTone::Warn,
            headline: format!("doze-kafka's data is {pct}% of the disk"),
            detail: format!(
                "{} of {}. See what holds it, and empty or delete what you no longer need.",
                bytes(total),
                bytes(space.total)
            ),
            topic: None,
        });
    }
    let big = u
        .topics
        .iter()
        .find(|t| t.keeps.unbounded() && t.bytes >= UNBOUNDED_BYTES)?;
    Some(DiskAlarm {
        tone: AlarmTone::Warn,
        headline: format!("{} keeps everything: {} so far", big.name, bytes(big.bytes)),
        detail: "It has no retention limit, so it grows for as long as anything writes to it. Give it a retention.ms, or empty it.".to_owned(),
        topic: Some(big.name.clone()),
    })
}

/// What a topic that sets no retention of its own keeps, for the line under the table.
#[must_use]
pub fn defaults_line(k: &Keeps) -> String {
    if k.unbounded() {
        "A topic that sets no retention of its own keeps everything: this broker was started without --retention-ms or --retention-bytes.".to_owned()
    } else {
        format!(
            "A topic that sets no retention of its own takes the broker's: {}",
            k.why(None)
        )
    }
}

/// Who reads the topic, for Empty topic: the groups with offsets committed on it, and what their next fetch meets.
#[must_use]
pub fn empty_in_use(u: &crate::model::InUse) -> Vec<crate::model::Span> {
    use crate::model::Span;
    if u.groups.is_empty() {
        return Vec::new();
    }
    let mut out = vec![Span::strong("It is read.")];
    for (i, (g, _, members)) in u.groups.iter().take(3).enumerate() {
        out.push(Span::text(if i == 0 { " Group " } else { ", group " }));
        out.push(Span::mono(g.clone()));
        out.push(Span::text(format!(
            " has offsets committed on it ({})",
            count(*members, "member", "members")
        )));
    }
    if u.groups.len() > 3 {
        out.push(Span::text(format!(
            ", and {} more groups",
            u.groups.len() - 3
        )));
    }
    out.push(Span::text(
        ". Their commits stay; their next fetch from one is out of range, and each starts again by its auto.offset.reset.",
    ));
    out
}

/// What Empty topic leaves as it was, line by line.
#[must_use]
pub fn empty_keeps(compacted: bool) -> Vec<String> {
    let mut lines = vec![
        "The topic, its partitions and its configs stay.".to_owned(),
        "Each partition carries on from its end: the next record gets the next offset, never one given before.".to_owned(),
        "Committed offsets stay as they are. A consumer at one is told OFFSET_OUT_OF_RANGE and starts again by its auto.offset.reset: earliest and latest are the same offset now.".to_owned(),
    ];
    if compacted {
        lines.push("The latest value of every key goes too. This is the broker's own action: Kafka's DeleteRecords is refused on a topic that only compacts.".to_owned());
    }
    lines
}

/// The button of Empty topic: what it frees.
#[must_use]
pub fn empty_button(v: &crate::model::EmptyTopicView) -> String {
    if v.bytes == 0 {
        "Empty topic".to_owned()
    } else {
        format!("Empty and free {}", bytes(v.bytes))
    }
}

/// What Erase everything removes, line by line.
#[must_use]
pub fn erase_removes() -> Vec<String> {
    vec![
        "Every topic, with its records and partitions. Clients see each one vanish, as with Delete topic.".to_owned(),
        "Every consumer group and its committed offsets. A group with members connected now stays, with nothing committed.".to_owned(),
        "Every transactional id whose transaction is over.".to_owned(),
        "Every subject and schema in the schema registry.".to_owned(),
        "Partition directories that belong to no topic.".to_owned(),
    ]
}

/// What Erase everything keeps, line by line.
#[must_use]
pub fn erase_keeps() -> Vec<String> {
    vec![
        "ACLs, and the users made over the API: who may use the broker is not data. Delete them on the Access page if you want them gone.".to_owned(),
        "The broker's dynamic configs, the flags it runs with, and the registry's global compatibility level.".to_owned(),
        "The producer id counter and the next schema id: an id is never given out twice, so a client that remembers one is not confused.".to_owned(),
    ]
}

/// What emptying one topic did.
#[must_use]
pub fn emptied(e: &Emptied) -> String {
    if e.offsets == 0 {
        return format!("{} was already empty: nothing to free.", e.topic);
    }
    format!(
        "Emptied {}: {} in {}, {} freed. It carries on from where it ended.",
        e.topic,
        count(
            usize::try_from(e.offsets).unwrap_or(usize::MAX),
            "record",
            "records"
        ),
        count(e.partitions, "partition", "partitions"),
        bytes(e.freed_bytes)
    )
}

/// What emptying every topic did.
#[must_use]
pub fn emptied_all(e: &EmptiedAll) -> String {
    format!(
        "Emptied {}: {}, {} freed. Every topic, partition and config is still there.",
        count(e.topics.len(), "topic", "topics"),
        count(
            usize::try_from(e.offsets()).unwrap_or(usize::MAX),
            "record",
            "records"
        ),
        bytes(e.freed_bytes())
    )
}

/// What erasing everything did (with the subjects the registry dropped, if there is one).
#[must_use]
pub fn erased(e: &Erased, subjects: Option<usize>) -> String {
    let mut gone = vec![
        count(e.topics, "topic", "topics"),
        count(e.groups, "group", "groups"),
        count(e.transactional_ids, "transactional id", "transactional ids"),
    ];
    if let Some(n) = subjects {
        gone.push(count(n, "schema subject", "schema subjects"));
    }
    let mut out = format!(
        "Erased {}; {} freed. ACLs, users and the broker's settings are as they were.",
        gone.join(", "),
        bytes(e.freed_bytes)
    );
    match e.groups_kept.as_slice() {
        [] => {}
        [one] => out.push_str(&format!(
            " The group {one} has members connected: it stays, with nothing committed."
        )),
        many => out.push_str(&format!(
            " {} groups have members connected: they stay, with nothing committed ({}).",
            many.len(),
            many.join(", ")
        )),
    }
    out
}

#[cfg(test)]
mod tests;
