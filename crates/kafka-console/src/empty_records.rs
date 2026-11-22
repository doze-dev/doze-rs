//! Empty a topic from its own page (CON-1590): every record of every partition up to its high watermark, in one DeleteRecords
//! call (offset -1, as `kafka-delete-records.sh` sends), the topic kept with its id, configs and partitions. What the dialog
//! shows (the view, read by `gather::empty_records`) and its words, made here so the API says them one way.
//!
//! Not the Data page's Empty (CON-1519), which is the broker's own action: it removes the segments at once and works on a
//! compacted topic too. DeleteRecords moves each log start; a topic whose cleanup.policy is only `compact` refuses it with
//! POLICY_VIOLATION, as Kafka does, and the dialog says so before the button and points at the Data page.

use crate::format::{bytes, number};
use crate::model::{Lines, Refusal, Span};

/// One partition: where its log starts and ends now, and what goes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct EmptyPart {
    pub partition: i32,
    pub log_start: i64,
    pub high_watermark: i64,
    /// Offsets between the two (records, as Delete records counts them: a compacted gap or a marker included).
    pub records: i64,
    /// The bytes of its segments on disk, when the broker could say.
    pub bytes: Option<u64>,
}

/// A group whose commits fall below the new log start.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct EmptyGroup {
    pub group: String,
    /// `(partition, committed offset)` of each commit below its partition's high watermark.
    pub below: Vec<(i32, i64)>,
    /// Records between those commits and the ends: what its consumers had not read yet, and will not.
    pub unread: i64,
    /// Its members now: a group with members cannot be moved (Kafka refuses an admin's commit to it).
    pub members: usize,
}

/// The Empty dialog of one topic.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct EmptyRecordsView {
    pub topic: String,
    pub parts: Vec<EmptyPart>,
    pub records: i64,
    pub bytes: Option<u64>,
    pub groups: Vec<EmptyGroup>,
    /// cleanup.policy is `compact` alone: the broker will refuse (POLICY_VIOLATION).
    pub compact_only: bool,
    /// The call that will be sent.
    pub call: Lines,
    /// What was wrong with what was sent (the name typed).
    pub error: Option<String>,
}

/// What became of one group asked to move to the new start.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GroupMoved {
    pub group: String,
    /// Partitions moved; 0 when refused.
    pub moved: usize,
    /// Why not, in Kafka's words (`UNKNOWN_MEMBER_ID`: the group has members).
    pub refused: Option<String>,
}

/// The topic was emptied.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RecordsEmptied {
    pub topic: String,
    /// `(partition, new log start)`.
    pub starts: Vec<(i32, i64)>,
    pub records: i64,
    pub moved: Vec<GroupMoved>,
}

/// What an Empty came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmptyResult {
    Done(RecordsEmptied),
    /// Kafka refused (the first partition's code); nothing was moved.
    Refused(Box<EmptyRecordsView>, Refusal),
    /// The name typed is not the topic's: the dialog again.
    Invalid(Box<EmptyRecordsView>),
    NoTopic,
}

fn plural(n: i64, one: &str, many: &str) -> String {
    format!("{} {}", number(n), if n == 1 { one } else { many })
}

/// "Removes 1,204 records (812.0 KiB on disk) from 6 partitions, each up to its high watermark."
#[must_use]
pub fn summary(v: &EmptyRecordsView) -> Vec<Span> {
    if v.records == 0 {
        return vec![Span::text(
            "Nothing to remove: every partition starts where it ends. The call would change nothing.",
        )];
    }
    let held = v.parts.iter().filter(|p| p.records > 0).count();
    let mut out = vec![
        Span::text(if v.compact_only { "Holds " } else { "Removes " }),
        Span::strong(plural(v.records, "record", "records")),
    ];
    if let Some(b) = v.bytes {
        out.push(Span::text(format!(" ({} on disk)", bytes(b))));
    }
    out.push(Span::text(if v.compact_only { " in " } else { " from " }));
    out.push(Span::strong(plural(
        i64::try_from(held).unwrap_or(0),
        "partition",
        "partitions",
    )));
    out.push(Span::text(if v.compact_only {
        ". DeleteRecords cannot remove them: the topic only compacts."
    } else {
        ", each up to its high watermark."
    }));
    out
}

/// What a group falling below the new start means, one sentence for all of them.
#[must_use]
pub fn groups_warning(v: &EmptyRecordsView) -> Vec<Span> {
    if v.groups.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, g) in v.groups.iter().enumerate() {
        if i > 0 {
            out.push(Span::text(if i + 1 == v.groups.len() {
                " and "
            } else {
                ", "
            }));
        }
        out.push(Span::mono(&g.group));
        out.push(Span::text(format!(
            " ({} unread on {})",
            number(g.unread),
            plural(
                i64::try_from(g.below.len()).unwrap_or(0),
                "partition",
                "partitions"
            )
        )));
    }
    out.push(Span::text(if v.groups.len() == 1 {
        " has commits below the new start: its consumers are told "
    } else {
        " have commits below the new start: their consumers are told "
    }));
    out.push(Span::mono("OFFSET_OUT_OF_RANGE"));
    out.push(Span::text(" and go where their "));
    out.push(Span::mono("auto.offset.reset"));
    out.push(Span::text(
        " says, unless they are moved to the new start with it.",
    ));
    out
}

/// What stays, the same for every topic but for the offsets it names.
#[must_use]
pub fn keeps(v: &EmptyRecordsView) -> Vec<String> {
    let mut out = vec!["The topic keeps its id, its configs and its partitions.".to_owned()];
    if let Some(p) = v.parts.first() {
        out.push(format!(
            "Offsets carry on from where each partition ends: the next record of {}-{} is offset {}.",
            v.topic,
            p.partition,
            number(p.high_watermark)
        ));
    }
    out.push("The records are no longer served at once; their segments leave the disk as each log start passes them. The Data page's Empty frees the disk now.".to_owned());
    out
}

/// The button's words.
#[must_use]
pub fn button(v: &EmptyRecordsView) -> String {
    if v.records == 0 {
        "Nothing to empty".to_owned()
    } else {
        format!("Empty {}", v.topic)
    }
}

/// The sentence of a refusal, after which the partitions are as they were.
#[must_use]
pub fn refused_sentence(r: &Refusal) -> String {
    if r.code == 44 {
        "A topic whose cleanup.policy is only compact refuses DeleteRecords, as Kafka does: nothing was removed. The Data page's Empty removes its records anyway.".to_owned()
    } else {
        format!(
            "Kafka refused the delete ({}): nothing was removed.",
            r.name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(records: &[i64], groups: Vec<EmptyGroup>) -> EmptyRecordsView {
        EmptyRecordsView {
            topic: "orders".into(),
            parts: records
                .iter()
                .enumerate()
                .map(|(i, r)| EmptyPart {
                    partition: i32::try_from(i).unwrap(),
                    log_start: 0,
                    high_watermark: *r,
                    records: *r,
                    bytes: Some(100),
                })
                .collect(),
            records: records.iter().sum(),
            bytes: Some(2048),
            groups,
            compact_only: false,
            call: Vec::new(),
            error: None,
        }
    }

    fn text(spans: &[Span]) -> String {
        spans
            .iter()
            .map(|s| match s {
                Span::Text(t) | Span::Mono(t) | Span::Strong(t) => t.as_str(),
            })
            .collect()
    }

    #[test]
    fn the_dialog_says_what_goes_who_falls_behind_and_what_stays() {
        let g = |n: &str, unread| EmptyGroup {
            group: n.into(),
            below: vec![(0, 1), (2, 0)],
            unread,
            members: 0,
        };
        let v = view(&[1200, 0, 4], vec![g("fulfilment", 1203), g("audit", 1)]);
        assert_eq!(
            text(&summary(&v)),
            "Removes 1,204 records (2.0 KiB on disk) from 2 partitions, each up to its high watermark."
        );
        assert_eq!(
            text(&groups_warning(&v)),
            "fulfilment (1,203 unread on 2 partitions) and audit (1 unread on 2 partitions) have commits below the new start: their consumers are told OFFSET_OUT_OF_RANGE and go where their auto.offset.reset says, unless they are moved to the new start with it."
        );
        assert!(keeps(&v)[1].contains("the next record of orders-0 is offset 1,200"));
        assert_eq!(button(&v), "Empty orders");
        let none = view(&[0, 0], Vec::new());
        assert_eq!(button(&none), "Nothing to empty");
        assert!(text(&summary(&none)).starts_with("Nothing to remove"));
        assert!(groups_warning(&none).is_empty());
        let mut compact = view(&[3], Vec::new());
        compact.compact_only = true;
        assert_eq!(
            text(&summary(&compact)),
            "Holds 3 records (2.0 KiB on disk) in 1 partition. DeleteRecords cannot remove them: the topic only compacts."
        );
    }
}
