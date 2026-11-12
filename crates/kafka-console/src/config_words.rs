//! The words of the Config page that are made from a key and a value: the small line under a value (`7 days`, `8 MiB`,
//! `no time limit`), the group a key is shown in, and the sentence under the page's name. Pure functions with tables of
//! cases in `config_words/tests.rs`.

use crate::format::{bytes, number, span_ms};
use crate::model::{ConfigRow, ConfigSource, Span};

/// Where a key is shown on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Cleanup,
    Retention,
    Segments,
    Records,
    Durability,
    Replication,
}

impl Group {
    /// The order the page shows the groups in.
    pub const ALL: [Self; 6] = [
        Self::Cleanup,
        Self::Retention,
        Self::Segments,
        Self::Records,
        Self::Durability,
        Self::Replication,
    ];

    /// The group's heading.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Cleanup => "Cleanup and compaction",
            Self::Retention => "Retention",
            Self::Segments => "Segments and index",
            Self::Records => "Records",
            Self::Durability => "Durability",
            Self::Replication => "Replication and tiered storage",
        }
    }
}

/// The group of a key. A key no group names is shown with the records.
#[must_use]
pub fn group_of(key: &str) -> Group {
    match key {
        "cleanup.policy"
        | "min.cleanable.dirty.ratio"
        | "min.compaction.lag.ms"
        | "max.compaction.lag.ms"
        | "delete.retention.ms" => Group::Cleanup,
        "retention.ms" | "retention.bytes" => Group::Retention,
        "segment.bytes"
        | "segment.ms"
        | "segment.jitter.ms"
        | "segment.index.bytes"
        | "index.interval.bytes"
        | "file.delete.delay.ms"
        | "preallocate" => Group::Segments,
        "flush.messages" | "flush.ms" => Group::Durability,
        "min.insync.replicas"
        | "unclean.leader.election.enable"
        | "leader.replication.throttled.replicas"
        | "follower.replication.throttled.replicas"
        | "local.retention.ms"
        | "local.retention.bytes"
        | "remote.storage.enable"
        | "remote.log.copy.disable"
        | "remote.log.delete.on.disable" => Group::Replication,
        _ => Group::Records,
    }
}

/// Whether a key belongs to the "Compaction" or "Segments" chips.
#[must_use]
pub fn is_compaction(key: &str) -> bool {
    group_of(key) == Group::Cleanup
}

/// Whether a key belongs to the "Segments" chip.
#[must_use]
pub fn is_segments(key: &str) -> bool {
    group_of(key) == Group::Segments
}

fn is_ms(key: &str) -> bool {
    key.ends_with(".ms")
}

fn is_size(key: &str) -> bool {
    key.ends_with(".bytes")
}

/// The small line under a value, in words: a duration, a size, "no limit", what a policy means. `None` when the value
/// says it itself.
#[must_use]
pub fn gloss(key: &str, value: &str) -> Option<String> {
    let n: i64 = value.trim().parse().ok()?;
    if key == "retention.ms" && n < 0 {
        return Some("no time limit".to_owned());
    }
    if key == "retention.bytes" && n < 0 {
        return Some("no size limit".to_owned());
    }
    if n == i64::MAX {
        return Some("no limit".to_owned());
    }
    if key == "min.compaction.lag.ms" && n == 0 {
        return Some("no waiting".to_owned());
    }
    if is_ms(key) && n >= 1000 {
        return Some(span_ms(n));
    }
    if is_size(key) && n >= 1024 {
        return Some(about_bytes(key, n));
    }
    None
}

fn about_bytes(key: &str, n: i64) -> String {
    let size = bytes(u64::try_from(n).unwrap_or(0)).replace(".0 ", " ");
    if key == "max.message.bytes" {
        format!("about {size}")
    } else {
        size
    }
}

/// What a cleanup policy means.
#[must_use]
pub fn policy_gloss(value: &str) -> Option<&'static str> {
    let kinds: Vec<&str> = value.split(',').map(str::trim).collect();
    let (compact, delete) = (kinds.contains(&"compact"), kinds.contains(&"delete"));
    match (compact, delete) {
        (true, true) => Some("keep the latest record of each key, and trim old segments"),
        (true, false) => Some("keep the latest record of each key"),
        (false, true) => Some("discard old segments"),
        (false, false) => None,
    }
}

/// The line under a row's value: the gloss, and for a value set on the topic that hides a flag, what the flag is.
#[must_use]
pub fn row_gloss(r: &ConfigRow) -> Option<String> {
    let own = if r.name == "cleanup.policy" {
        policy_gloss(&r.value).map(str::to_owned)
    } else {
        gloss(&r.name, &r.value)
    };
    let flag = (r.source == ConfigSource::DynamicTopic)
        .then(|| {
            r.synonyms
                .iter()
                .find(|s| s.source == ConfigSource::StaticFlag)
        })
        .flatten()
        .map(|s| {
            format!(
                "the flag is {}",
                gloss(&r.name, &s.value).unwrap_or_else(|| s.value.clone())
            )
        });
    match (own, flag) {
        (Some(a), Some(b)) => Some(format!("{a}; {b}")),
        (a, b) => a.or(b),
    }
}

/// How many rows have each source: `(set on the topic, from a flag, Kafka's defaults)`.
#[must_use]
pub fn tally(rows: &[ConfigRow]) -> (usize, usize, usize) {
    let count = |s: ConfigSource| rows.iter().filter(|r| r.source == s).count();
    (
        count(ConfigSource::DynamicTopic),
        count(ConfigSource::StaticFlag),
        count(ConfigSource::Default),
    )
}

/// The sentence under the topic's name on the Config page.
#[must_use]
pub fn page_sentence(rows: &[ConfigRow], moment: &str) -> Vec<Span> {
    let (topic, flag, default) = tally(rows);
    let mut out = vec![
        Span::strong(number(i64::try_from(rows.len()).unwrap_or(i64::MAX))),
        Span::text(" configs. "),
        Span::strong(number(i64::try_from(topic).unwrap_or(0))),
        Span::text(" are set on this topic, "),
        Span::strong(number(i64::try_from(flag).unwrap_or(0))),
        Span::text(if flag == 1 { " comes" } else { " come" }),
        Span::text(" from a broker flag, "),
        Span::strong(number(i64::try_from(default).unwrap_or(0))),
        Span::text(if default == 1 { " is" } else { " are" }),
        Span::text(" Kafka's defaults."),
    ];
    if !moment.is_empty() {
        out.push(Span::text(format!(" {moment}")));
    }
    out
}

/// Kafka's documentation string as plain text: tags removed, whitespace collapsed. Never markup.
#[must_use]
pub fn plain_text(doc: &str) -> String {
    let mut text = String::with_capacity(doc.len());
    let mut in_tag = false;
    for c in doc.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            _ => text.push(c),
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The first sentence of Kafka's text, as the row of the page says it.
#[must_use]
pub fn first_sentence(doc: &str) -> String {
    let text = plain_text(doc);
    match text.find(". ") {
        Some(at) => text.get(..=at).unwrap_or(&text).to_owned(),
        None => text,
    }
}

/// Whether Kafka's text says more than its first sentence.
#[must_use]
pub fn has_more(doc: &str) -> bool {
    plain_text(doc).len() > first_sentence(doc).len()
}

/// The type DescribeConfigs reports, in a word.
#[must_use]
pub const fn type_word(kind: i8) -> &'static str {
    match kind {
        1 => "boolean",
        2 => "string",
        3 => "int",
        5 => "long",
        6 => "double",
        7 => "list",
        _ => "other",
    }
}

#[cfg(test)]
mod tests;
