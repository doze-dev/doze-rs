//! Topic configs: which keys exist, what values they take, and what a topic's configuration is once the
//! broker's defaults are laid under its own.
//!
//! A key is accepted only if it is a real Kafka topic config, and its value only if Kafka would accept it (the
//! wording of a refusal is Kafka's, recorded from 4.3.1). Accepting is not acting: which of them the broker acts
//! on is in `docs/limitations.md`.

mod cleanup;
mod docs;
mod table;

use std::collections::BTreeMap;

use kafka_storage::{LogConfig, Retention};

use crate::Defaults;
pub(crate) use cleanup::{cleaner, compacts, compacts_only};
use table::{Fallback, Kind, SETTINGS, Setting};

/// Where a config's value comes from, as DescribeConfigs and CreateTopics number them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// Set on the topic.
    DynamicTopic,
    /// The broker's own setting, which is not Kafka's default (a flag it was started with).
    StaticBroker,
    /// Kafka's default.
    Default,
}

impl Source {
    /// The number on the wire.
    #[must_use]
    pub(crate) fn code(self) -> i8 {
        match self {
            Self::DynamicTopic => 1,
            Self::StaticBroker => 4,
            Self::Default => 5,
        }
    }
}

/// One config of a topic as it is in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The key.
    pub name: &'static str,
    /// The value.
    pub value: String,
    /// Where it comes from.
    pub source: Source,
    /// The type, as DescribeConfigs numbers it (BOOLEAN 1, STRING 2, INT 3, LONG 5, DOUBLE 6, LIST 7).
    pub kind: i8,
    /// What Kafka says it is for.
    pub doc: &'static str,
}

/// The first config of `configs` (in name order, so the answer is stable) that is not a topic config or whose value
/// Kafka would refuse, as the message Kafka gives.
///
/// # Errors
/// The refusal's message.
pub(crate) fn validate(configs: &BTreeMap<String, String>) -> Result<(), String> {
    for (name, value) in configs {
        let setting = lookup(name).ok_or_else(|| format!("Unknown topic config name: {name}"))?;
        setting
            .kind
            .check(value)
            .map_err(|why| format!("Invalid value {value} for configuration {name}: {why}"))?;
    }
    let lag = |name: &str| configs.get(name).and_then(|v| v.trim().parse::<i64>().ok());
    if let (Some(min), Some(max)) = (lag("min.compaction.lag.ms"), lag("max.compaction.lag.ms"))
        && min > max
    {
        return Err(format!(
            "conflict topic config setting min.compaction.lag.ms ({min}) > max.compaction.lag.ms ({max})"
        ));
    }
    Ok(())
}

/// A topic's configuration in force: every config that has a default, with the topic's own laid over them, by
/// name.
#[must_use]
pub(crate) fn effective(defaults: &Defaults, own: &BTreeMap<String, String>) -> Vec<Entry> {
    let mut all: Vec<Entry> = SETTINGS
        .iter()
        .filter_map(|s| {
            if let Some((_, value)) = own.get_key_value(s.name) {
                return Some(entry(s, value.clone(), Source::DynamicTopic));
            }
            let (value, source) = match &s.default {
                Fallback::None => return None,
                Fallback::Fixed(v) => ((*v).to_owned(), Source::Default),
                Fallback::Broker(f, kafka) => {
                    let value = f(defaults);
                    let source = if value == *kafka {
                        Source::Default
                    } else {
                        Source::StaticBroker
                    };
                    (value, source)
                }
            };
            Some(entry(s, value, source))
        })
        .collect();
    // In the order Kafka lists them (a Java hash map's), then any it does not list.
    all.sort_by_key(|e| {
        docs::ORDER
            .iter()
            .position(|n| *n == e.name)
            .unwrap_or(usize::MAX)
    });
    all
}

fn entry(s: &'static Setting, value: String, source: Source) -> Entry {
    Entry {
        name: s.name,
        value,
        source,
        kind: s.kind.wire_type(s.name),
        doc: docs::DOCS
            .iter()
            .find(|(n, _)| *n == s.name)
            .map_or("", |(_, d)| d),
    }
}

/// Whether the config is a list, which APPEND and SUBTRACT may be used on.
#[must_use]
pub(crate) fn is_list(name: &str) -> bool {
    lookup(name).is_some_and(|s| s.kind.wire_type(s.name) == 7)
}

/// What the topic config is when the topic does not set it (for APPEND to start from), if it has a default.
#[must_use]
pub(crate) fn default_of(defaults: &Defaults, name: &str) -> Option<String> {
    effective(defaults, &BTreeMap::new())
        .into_iter()
        .find(|e| e.name == name)
        .map(|e| e.value)
}

/// How much of a partition the topic keeps: its own `retention.bytes` and `retention.ms` (a negative one is no bound), else the
/// broker's.
pub(crate) fn retention(defaults: &Defaults, own: &BTreeMap<String, String>) -> Retention {
    let pick = |key: &str, default: i64| {
        let n = own
            .get(key)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(default);
        (n >= 0).then_some(n)
    };
    Retention {
        max_bytes: pick("retention.bytes", defaults.retention_bytes)
            .and_then(|n| u64::try_from(n).ok()),
        max_age_ms: pick("retention.ms", defaults.retention_ms),
    }
}

/// Kafka's default `segment.ms`: a week.
const DEFAULT_SEGMENT_MS: i64 = 604_800_000;

/// How a partition's log is sized: the topic's own `segment.bytes` and `index.interval.bytes`, else the broker's.
pub(crate) fn log_config(defaults: &Defaults, own: &BTreeMap<String, String>) -> LogConfig {
    let pick = |key: &str, default: i64| {
        let n = own
            .get(key)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(default);
        u64::try_from(n).ok().filter(|n| *n > 0).unwrap_or(1)
    };
    // `segment.ms` is a topic's own or Kafka's default of a week (`log.roll.ms`, `log.roll.hours`: not set on a broker here).
    let segment_ms = own
        .get("segment.ms")
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_SEGMENT_MS);
    LogConfig {
        segment_bytes: pick("segment.bytes", defaults.segment_bytes),
        index_interval_bytes: pick("index.interval.bytes", defaults.index_interval_bytes),
        segment_ms: Some(segment_ms),
    }
}

fn lookup(name: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.name == name)
}

impl Kind {
    /// The type DescribeConfigs reports: BOOLEAN 1, STRING 2, INT 3, LONG 5, DOUBLE 6, LIST 7.
    fn wire_type(&self, name: &str) -> i8 {
        match self {
            Self::Bool => 1,
            Self::Int { .. } => 3,
            Self::Long { .. } => 5,
            Self::Double { .. } => 6,
            Self::CleanupPolicy => 7,
            Self::Text if name.ends_with(".throttled.replicas") => 7,
            Self::Choice(_) | Self::Text => 2,
        }
    }

    fn check(&self, v: &str) -> Result<(), String> {
        match self {
            Self::Int { min } => number::<i32>(v, "INT", *min),
            Self::Long { min } => number::<i64>(v, "LONG", *min),
            Self::Double { min, max } => {
                let f: f64 = v
                    .parse()
                    .map_err(|_| "Not a number of type DOUBLE".to_owned())?;
                if f < *min {
                    Err(format!("Value must be at least {min}"))
                } else if f > *max {
                    Err(format!("Value must be no more than {max}"))
                } else {
                    Ok(())
                }
            }
            Self::Bool => one_of(v, &["true", "false"]),
            Self::Choice(allowed) => one_of(v, allowed),
            // Kafka checks each element of the list and nothing else, so repeats are fine.
            Self::CleanupPolicy => v
                .split(',')
                .try_for_each(|p| one_of(p.trim(), &["compact", "delete"])),
            Self::Text => Ok(()),
        }
    }
}

fn number<T: std::str::FromStr + Into<i64>>(v: &str, ty: &str, min: i64) -> Result<(), String> {
    let n: i64 = v
        .parse::<T>()
        .map_err(|_| format!("Not a number of type {ty}"))?
        .into();
    if n < min {
        return Err(format!("Value must be at least {min}"));
    }
    Ok(())
}

fn one_of(v: &str, allowed: &[&str]) -> Result<(), String> {
    if allowed.contains(&v) {
        Ok(())
    } else {
        Err(format!("String must be one of: {}", allowed.join(", ")))
    }
}

#[cfg(test)]
mod tests;

/// The name of a topic config as this table spells it, if it is one.
#[must_use]
pub(crate) fn lookup_name(name: &str) -> Option<&'static str> {
    lookup(name).map(|s| s.name)
}

/// What Kafka's own default for a topic config is, as text, if it has one.
#[must_use]
pub(crate) fn kafka_default_of(name: &str) -> Option<String> {
    match lookup(name)?.default {
        Fallback::None => None,
        Fallback::Fixed(v) | Fallback::Broker(_, v) => Some(v.to_owned()),
    }
}

/// What a value of the topic config may be, in words, if it is one.
#[must_use]
#[cfg_attr(not(feature = "inspect"), allow(dead_code))]
pub(crate) fn allowed_of(name: &str) -> Option<String> {
    lookup(name).map(|s| s.kind.allowed())
}
