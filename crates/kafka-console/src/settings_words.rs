//! The words of the Settings page: what each dynamic broker config is for and when a change counts, which family each
//! API is listed under, which flag sets a config this broker does not change while it runs, and the documented limits of
//! the weight. Pure functions with tables of cases; nothing here asks the broker anything.

use std::time::Duration;

use kafka_wire::messages::ApiKey;

use crate::config_effects::{Moment, moment, pace_word};
use crate::format::{bytes, number, span};

/// The five broker configs that act on a running broker, the topic config each is the default of, and what it is for.
pub const DYNAMIC: [(&str, &str, &str); 5] = [
    (
        "log.segment.bytes",
        "segment.bytes",
        "A partition rolls to a new segment past this size",
    ),
    (
        "log.index.interval.bytes",
        "index.interval.bytes",
        "One sparse index entry per this many bytes",
    ),
    (
        "message.max.bytes",
        "max.message.bytes",
        "The largest record batch a topic accepts",
    ),
    (
        "log.retention.bytes",
        "retention.bytes",
        "Delete the oldest segments past this size per partition",
    ),
    (
        "log.retention.ms",
        "retention.ms",
        "Delete segments older than this",
    ),
];

/// What a value of each may be, in words (the broker's own checks: `configs/broker.rs`; a live test holds them together).
#[must_use]
pub fn allowed(key: &str) -> &'static str {
    match key {
        "log.segment.bytes" => "At least 1,048,576 (1 MiB) and at most 2,147,483,647.",
        "log.index.interval.bytes" | "message.max.bytes" => "From 0 to 2,147,483,647.",
        "log.retention.bytes" => "Any number of bytes; -1 means no limit.",
        "log.retention.ms" => "-1 or any larger number of milliseconds.",
        _ => "",
    }
}

/// The topic config a broker default lies under.
#[must_use]
pub fn topic_key(key: &str) -> Option<&'static str> {
    DYNAMIC.iter().find(|d| d.0 == key).map(|d| d.1)
}

/// The flag whose value a dynamic config starts from (`--segment-bytes` for `log.segment.bytes`).
#[must_use]
pub fn dynamic_flag(key: &str) -> Option<&'static str> {
    Some(match key {
        "log.segment.bytes" => "--segment-bytes",
        "log.index.interval.bytes" => "--index-interval-bytes",
        "message.max.bytes" => "--message-max-bytes",
        "log.retention.bytes" => "--retention-bytes",
        "log.retention.ms" => "--retention-ms",
        _ => return None,
    })
}

/// The dynamic config a flag is the start value of.
#[must_use]
pub fn flag_dynamic(flag: &str) -> Option<&'static str> {
    DYNAMIC
        .iter()
        .map(|d| d.0)
        .find(|k| dynamic_flag(k) == Some(flag))
}

/// Whether `key` is one of the five.
#[must_use]
pub fn is_dynamic(key: &str) -> bool {
    topic_key(key).is_some()
}

/// What the key is for.
#[must_use]
pub fn what(key: &str) -> &'static str {
    DYNAMIC.iter().find(|d| d.0 == key).map_or("", |d| d.2)
}

/// When a change counts, as the Settings table writes it. `retention_secs` is the pace of the retention pass, where known.
#[must_use]
pub fn takes_effect(key: &str, retention_secs: Option<u64>) -> String {
    match topic_key(key).and_then(moment) {
        Some(Moment::NextAppend) => "At the next append of each open partition".to_owned(),
        Some(Moment::NextRequest) => "On the next request".to_owned(),
        Some(Moment::RetentionPass) => retention_secs.map_or_else(
            || "At the next retention pass".to_owned(),
            |s| format!("At the next retention pass, every {}", pace_word(s)),
        ),
        Some(Moment::CleanerLook) => "At the cleaner's next look".to_owned(),
        _ => "Not acted on".to_owned(),
    }
}

/// The value in words: `128 MiB`, `no limit`, `3 d`, `1 MiB + 12 B`.
#[must_use]
pub fn gloss(key: &str, value: &str) -> Option<String> {
    let n: i64 = value.trim().parse().ok()?;
    if n == -1 && matches!(key, "log.retention.ms" | "log.retention.bytes") {
        return Some("no limit".to_owned());
    }
    if key == "message.max.bytes" && n > 12 && (n - 12) % 1024 == 0 {
        let size = bytes(u64::try_from(n - 12).ok()?).replace(".0 ", " ");
        return Some(format!("{size} + 12 B"));
    }
    crate::config_words::gloss(topic_key(key)?, value)
}

/// The unit under the field while a value is typed: `ms = 3 d`, `B = 128 MiB`.
#[must_use]
pub fn unit(key: &str, typed: &str) -> Option<String> {
    let g = gloss(key, typed)?;
    let u = if key.ends_with(".ms") { "ms" } else { "B" };
    Some(format!("{u} = {g}"))
}

/// The flag that sets a config this broker holds and does not change while it runs.
#[must_use]
pub fn flag_of(key: &str) -> Option<&'static str> {
    Some(match key {
        "num.partitions" => "--default-partitions",
        "auto.create.topics.enable" => "--auto-create-topics",
        "log.dirs" => "--data-dir",
        "listeners" => "--listen",
        "advertised.listeners" => "--advertised-host",
        "log.retention.check.interval.ms" => "--log-retention-check-interval",
        "group.initial.rebalance.delay.ms" => "--group-initial-rebalance-delay",
        "offsets.retention.minutes" => "--offsets-retention",
        "group.consumer.session.timeout.ms" => "--consumer-session-timeout",
        _ => return None,
    })
}

/// A duration as the flags write it: `5m`, `15s`, `500ms`, `168h`.
#[must_use]
pub fn flag_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms == 0 {
        "0".to_owned()
    } else if ms.is_multiple_of(3_600_000) {
        format!("{}h", ms / 3_600_000)
    } else if ms.is_multiple_of(60_000) {
        format!("{}m", ms / 60_000)
    } else if ms.is_multiple_of(1000) {
        format!("{}s", ms / 1000)
    } else {
        format!("{ms}ms")
    }
}

/// A duration in words, for the sentence under the intervals: `5 m`, `15 s`, `500 ms`.
#[must_use]
pub fn spoken(d: Duration) -> String {
    if d.as_millis() < 1000 {
        format!("{} ms", d.as_millis())
    } else {
        pace_word(d.as_secs())
    }
}

/// How the broker's own default for a topic's own value reads when the topic sets none: for the "who sees it" lines.
#[must_use]
pub fn seen_as(key: &str, value: &str) -> String {
    let n = value.trim().parse::<i64>().ok();
    match (key, n) {
        ("log.retention.ms", Some(ms)) if ms >= 1000 => span(ms / 1000),
        (
            "log.segment.bytes"
            | "message.max.bytes"
            | "log.retention.bytes"
            | "log.index.interval.bytes",
            Some(b),
        ) if b >= 1024 => bytes(u64::try_from(b).unwrap_or(0)).replace(".0 ", " "),
        (_, Some(v)) => number(v),
        _ => value.to_owned(),
    }
}

/// The families of the version profile, in the order the page lists them.
const FAMILIES: [(&str, &[ApiKey]); 8] = [
    (
        "Produce and fetch",
        &[
            ApiKey::Produce,
            ApiKey::Fetch,
            ApiKey::ListOffsets,
            ApiKey::DeleteRecords,
        ],
    ),
    (
        "Metadata and bootstrapping",
        &[
            ApiKey::ApiVersions,
            ApiKey::Metadata,
            ApiKey::FindCoordinator,
            ApiKey::DescribeCluster,
        ],
    ),
    (
        "Topics and configs",
        &[
            ApiKey::CreateTopics,
            ApiKey::DeleteTopics,
            ApiKey::CreatePartitions,
            ApiKey::DescribeConfigs,
            ApiKey::AlterConfigs,
            ApiKey::IncrementalAlterConfigs,
        ],
    ),
    (
        "Classic groups",
        &[
            ApiKey::JoinGroup,
            ApiKey::SyncGroup,
            ApiKey::Heartbeat,
            ApiKey::LeaveGroup,
            ApiKey::DescribeGroups,
            ApiKey::ListGroups,
            ApiKey::DeleteGroups,
        ],
    ),
    (
        "Consumer groups (KIP-848) and offsets",
        &[
            ApiKey::ConsumerGroupHeartbeat,
            ApiKey::ConsumerGroupDescribe,
            ApiKey::OffsetCommit,
            ApiKey::OffsetFetch,
            ApiKey::OffsetDelete,
        ],
    ),
    (
        "Producers and transactions",
        &[
            ApiKey::InitProducerId,
            ApiKey::AddPartitionsToTxn,
            ApiKey::AddOffsetsToTxn,
            ApiKey::EndTxn,
            ApiKey::TxnOffsetCommit,
            ApiKey::DescribeProducers,
            ApiKey::DescribeTransactions,
            ApiKey::ListTransactions,
        ],
    ),
    (
        "Security: ACLs and users",
        &[
            ApiKey::DescribeAcls,
            ApiKey::CreateAcls,
            ApiKey::DeleteAcls,
            ApiKey::DescribeUserScramCredentials,
            ApiKey::AlterUserScramCredentials,
            ApiKey::SaslHandshake,
            ApiKey::SaslAuthenticate,
        ],
    ),
    (
        "Client telemetry",
        &[ApiKey::GetTelemetrySubscriptions, ApiKey::PushTelemetry],
    ),
];

/// The family an API is listed under and its place in the page's order; `None` for an API the table does not know (a
/// test fails when the broker serves one).
#[must_use]
pub fn family_of(key: ApiKey) -> Option<(usize, &'static str)> {
    let mut place = 0;
    for (name, keys) in FAMILIES {
        if let Some(i) = keys.iter().position(|k| *k == key) {
            return Some((place + i, name));
        }
        place += keys.len();
    }
    None
}

/// `v3 to v9`, or `v0 only`.
#[must_use]
pub fn versions(min: i16, max: i16) -> String {
    if min == max {
        format!("v{min} only")
    } else {
        format!("v{min} to v{max}")
    }
}

#[cfg(test)]
mod tests;
