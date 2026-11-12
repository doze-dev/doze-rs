//! The words of a row: its target, the second line under it, and what its result adds. Pure functions of the row's facts.

use kafka_wire::messages::ApiKey;

use super::errors::{self, Class};
use super::facts::Facts;
use super::row::Row;
use crate::format::{bytes, number};

/// The first topic and partition as `orders-0`, or the topic alone when several partitions were named.
fn first_target(f: &Facts) -> Option<(String, Option<String>)> {
    let t = f.topics.first()?;
    let more = f.topics.len() + usize::try_from(f.topics_more).unwrap_or(0) - 1;
    let sub = if more > 0 {
        Some(format!("and {more} more"))
    } else if t.partitions.len() + usize::try_from(t.more).unwrap_or(0) > 1 {
        Some(format!(
            "{} partitions",
            t.partitions.len() + usize::try_from(t.more).unwrap_or(0)
        ))
    } else {
        None
    };
    let name = match (t.partitions.as_slice(), t.more) {
        ([p], 0) => format!("{}-{p}", t.name),
        _ => t.name.to_string(),
    };
    Some((name, sub))
}

/// The target column: what the request touched, and a second line.
#[must_use]
pub fn target(row: &Row) -> (String, Option<String>) {
    let f = &*row.facts;
    if let Some(group) = &f.group {
        let sub = match (f.generation, &f.member) {
            (Some(g), _) if g >= 0 => Some(format!("generation {g}")),
            (_, Some(m)) => Some(format!("by {m}")),
            _ => None,
        };
        // Offsets and commits name a group and topics: the group leads, the topics are on the detail.
        return (group.to_string(), sub);
    }
    if let Some((name, sub)) = first_target(f) {
        let sub = match row.api {
            ApiKey::Fetch => f.from_offset.map(|o| format!("from {}", number(o))),
            _ => sub,
        };
        return (name, sub);
    }
    if let Some(t) = &f.txn_id {
        return (t.to_string(), None);
    }
    (
        f.what
            .as_deref()
            .map_or_else(|| "-".to_owned(), str::to_owned),
        None,
    )
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!(
        "{} {}",
        number(i64::from(n)),
        if n == 1 { one } else { many }
    )
}

/// What an answered request adds: `1 record`, `1 batch, 211 B`, `9 topics`; `None` when there is nothing to add.
#[must_use]
pub fn done(row: &Row) -> Option<String> {
    let f = &*row.facts;
    let each = row.count > 1;
    match row.api {
        ApiKey::Produce => {
            let n = if each {
                f.records / row.count.max(1)
            } else {
                f.records
            };
            let word = plural(n, "record", "records");
            Some(if each {
                format!("about {word} each")
            } else {
                word
            })
        }
        ApiKey::Fetch if f.records == 0 && f.batches == 0 => {
            Some("caught up, no records".to_owned())
        }
        ApiKey::Fetch => {
            let (b, size) = if each {
                (
                    f.batches / row.count.max(1),
                    f.bytes / u64::from(row.count.max(1)),
                )
            } else {
                (f.batches, f.bytes)
            };
            let tail = if each { " each" } else { "" };
            Some(format!(
                "{}, {}{tail}",
                plural(b, "batch", "batches"),
                bytes(size)
            ))
        }
        ApiKey::Metadata => Some(plural(f.items, "topic", "topics")),
        ApiKey::ListGroups | ApiKey::DescribeGroups | ApiKey::ConsumerGroupDescribe => {
            Some(plural(f.items, "group", "groups"))
        }
        ApiKey::ApiVersions => f.what.as_deref().map(str::to_owned),
        ApiKey::DescribeConfigs => Some(plural(f.items, "config", "configs")),
        ApiKey::JoinGroup => f.reason.as_ref().map(|r| format!("reason: {r}")),
        ApiKey::LeaveGroup => f.reason.as_ref().map(|r| format!("reason: {r}")),
        ApiKey::EndTxn => f.what.as_deref().map(str::to_owned),
        _ => None,
    }
}

/// What a refused request says under its code: the broker's own message if it gave one, else a size for a batch that was too
/// large, else our plain sentence for the code.
#[must_use]
pub fn refused(row: &Row) -> String {
    let f = &*row.facts;
    if let Some(note) = f.record_note.as_ref().or(f.note.as_ref()) {
        return note.to_string();
    }
    if row.code == 10 {
        return format!("a request of {}", bytes(u64::from(row.request_bytes)));
    }
    errors::of(row.code).map_or_else(String::new, |r| r.plain.to_owned())
}

/// The class's word.
#[must_use]
pub fn word(class: Class, closed: bool) -> &'static str {
    match (class, closed) {
        (_, true) => "Closed",
        (Class::Ok, _) => "Done",
        (Class::Warn, _) => "Told to retry",
        (Class::Bad, _) => "Failed",
        (Class::Refused, _) => "Refused",
        (Class::Denied, _) => "Denied",
    }
}

/// The second line of the took column.
#[must_use]
pub fn took_sub(row: &Row) -> Option<String> {
    if row.count > 1 {
        return Some(if row.tooks.len() >= super::row::SAMPLES {
            "median of the last 64".to_owned()
        } else {
            "median".to_owned()
        });
    }
    match (row.api, row.facts.wait_ms) {
        (ApiKey::Fetch, Some(w)) if w > 0 => {
            Some(format!("willing to wait {} ms", number(i64::from(w))))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
