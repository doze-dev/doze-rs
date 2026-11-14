//! The configs of a topic that the broker acts on as it stores each batch, read from the topic's config map once, when
//! the topic is made or its configs change, rather than for every partition of every Produce.
//!
//! Values are taken as the broker has accepted them (`kafka-broker`'s `topic_config::validate`); a value that does not
//! read is as good as unset, and the broker's default applies.

use std::collections::BTreeMap;

use kafka_wire::batch::{Compression, TimestampType};

/// A topic's own settings, typed. An unset number is `None`: the broker's default applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopicSettings {
    /// `compression.type`: the codec its batches are stored in; `None` keeps the producer's (`producer`, or unset).
    pub compression: Option<Compression>,
    /// `message.timestamp.type`: whose clock its records carry.
    pub timestamp_type: TimestampType,
    /// `max.message.bytes`.
    pub max_message_bytes: Option<i64>,
    /// `message.timestamp.before.max.ms`.
    pub timestamp_before_max_ms: Option<i64>,
    /// `message.timestamp.after.max.ms`.
    pub timestamp_after_max_ms: Option<i64>,
    /// `cleanup.policy`.
    pub cleanup: CleanupPolicy,
}

/// What `cleanup.policy` names: compaction, deletion (retention), both, or neither (an empty list keeps everything).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanupPolicy {
    pub compact: bool,
    pub delete: bool,
}

impl CleanupPolicy {
    /// The policy a `cleanup.policy` value names; unset is Kafka's default, `delete`.
    #[must_use]
    pub fn parse(value: Option<&str>) -> Self {
        let Some(value) = value else {
            return Self {
                compact: false,
                delete: true,
            };
        };
        let names = |kind: &str| value.split(',').any(|k| k.trim().eq_ignore_ascii_case(kind));
        Self {
            compact: names("compact"),
            delete: names("delete"),
        }
    }

    /// Compacted and never deleted by retention: DeleteRecords is refused, and retention does not touch the log.
    #[must_use]
    pub fn compacts_only(self) -> bool {
        self.compact && !self.delete
    }
}

impl TopicSettings {
    /// The settings `config` names.
    #[must_use]
    pub fn of(config: &BTreeMap<String, String>) -> Self {
        let number = |key: &str| config.get(key).and_then(|v| v.parse::<i64>().ok());
        Self {
            compression: match config.get("compression.type").map(String::as_str) {
                Some("uncompressed") => Some(Compression::None),
                Some("gzip") => Some(Compression::Gzip),
                Some("snappy") => Some(Compression::Snappy),
                Some("lz4") => Some(Compression::Lz4),
                Some("zstd") => Some(Compression::Zstd),
                _ => None,
            },
            timestamp_type: if config
                .get("message.timestamp.type")
                .is_some_and(|v| v == "LogAppendTime")
            {
                TimestampType::LogAppend
            } else {
                TimestampType::Create
            },
            max_message_bytes: number("max.message.bytes"),
            timestamp_before_max_ms: number("message.timestamp.before.max.ms"),
            timestamp_after_max_ms: number("message.timestamp.after.max.ms"),
            cleanup: CleanupPolicy::parse(config.get("cleanup.policy").map(String::as_str)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(pairs: &[(&str, &str)]) -> TopicSettings {
        TopicSettings::of(
            &pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        )
    }

    #[test]
    fn an_unset_or_unreadable_setting_leaves_the_brokers_default() {
        let none = of(&[]);
        assert_eq!(none.compression, None);
        assert_eq!(none.timestamp_type, TimestampType::Create);
        assert_eq!(none.max_message_bytes, None);
        assert!(none.cleanup.delete && !none.cleanup.compact);
        let odd = of(&[("compression.type", "producer"), ("max.message.bytes", "lots")]);
        assert_eq!((odd.compression, odd.max_message_bytes), (None, None));
    }

    #[test]
    fn each_setting_reads_as_kafka_spells_it() {
        let s = of(&[
            ("compression.type", "uncompressed"),
            ("message.timestamp.type", "LogAppendTime"),
            ("max.message.bytes", "1000"),
            ("message.timestamp.before.max.ms", "5"),
            ("message.timestamp.after.max.ms", "6"),
        ]);
        assert_eq!(s.compression, Some(Compression::None));
        assert_eq!(s.timestamp_type, TimestampType::LogAppend);
        assert_eq!(
            (s.max_message_bytes, s.timestamp_before_max_ms, s.timestamp_after_max_ms),
            (Some(1000), Some(5), Some(6))
        );
        for (name, codec) in [
            ("gzip", Compression::Gzip),
            ("snappy", Compression::Snappy),
            ("lz4", Compression::Lz4),
            ("zstd", Compression::Zstd),
        ] {
            assert_eq!(of(&[("compression.type", name)]).compression, Some(codec));
        }
    }

    #[test]
    fn a_cleanup_policy_is_a_list_of_kinds() {
        let p = |v: &str| CleanupPolicy::parse(Some(v));
        assert!(p("compact").compacts_only());
        assert!(!p("compact,delete").compacts_only() && p("compact,delete").compact);
        assert!(p(" delete , COMPACT").compact, "trimmed, in any case");
        assert!(!p("delete").compact && !p("delete").compacts_only());
        let empty = p("");
        assert!(!empty.compact && !empty.delete, "an empty list: neither");
        assert!(!CleanupPolicy::parse(None).compacts_only());
    }
}
