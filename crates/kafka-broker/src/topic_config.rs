//! Topic configs: which keys exist, what values they take, and what a topic's configuration is once the
//! broker's defaults are laid under its own.
//!
//! A key is accepted only if it is a real Kafka topic config, and its value only if Kafka would accept it (the
//! wording of a refusal is Kafka's, recorded from 4.3.1). Accepting is not acting: which of them the broker acts
//! on is in `docs/limitations.md`.

mod table;

use std::collections::BTreeMap;

use kafka_storage::{LogConfig, Retention};

use crate::Defaults;
use table::{Fallback, Kind, SETTINGS, Setting};

/// Where a config's value comes from, as DescribeConfigs and CreateTopics number them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// Set on the topic.
    DynamicTopic,
    /// The broker's default.
    Default,
}

impl Source {
    /// The number on the wire.
    #[must_use]
    pub(crate) fn code(self) -> i8 {
        match self {
            Self::DynamicTopic => 1,
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
                return Some(Entry {
                    name: s.name,
                    value: value.clone(),
                    source: Source::DynamicTopic,
                });
            }
            let value = match &s.default {
                Fallback::None => return None,
                Fallback::Fixed(v) => (*v).to_owned(),
                Fallback::Broker(f) => f(defaults),
            };
            Some(Entry {
                name: s.name,
                value,
                source: Source::Default,
            })
        })
        .collect();
    all.sort_by(|a, b| a.name.cmp(b.name));
    all
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

/// How a partition's log is sized: the topic's own `segment.bytes` and `index.interval.bytes`, else the broker's.
pub(crate) fn log_config(defaults: &Defaults, own: &BTreeMap<String, String>) -> LogConfig {
    let pick = |key: &str, default: i64| {
        let n = own
            .get(key)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(default);
        u64::try_from(n).ok().filter(|n| *n > 0).unwrap_or(1)
    };
    LogConfig {
        segment_bytes: pick("segment.bytes", defaults.segment_bytes),
        index_interval_bytes: pick("index.interval.bytes", defaults.index_interval_bytes),
    }
}

fn lookup(name: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.name == name)
}

impl Kind {
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

/// Whether the topic only compacts: its policy names `compact` and not `delete`. Deleting records from such a topic is
/// refused, and retention does not touch it.
pub(crate) fn compacts_only(own: &BTreeMap<String, String>) -> bool {
    own.get("cleanup.policy").is_some_and(|p| {
        let kinds: Vec<_> = p
            .split(',')
            .map(|k| k.trim().to_ascii_lowercase())
            .collect();
        kinds.iter().any(|k| k == "compact") && !kinds.iter().any(|k| k == "delete")
    })
}
