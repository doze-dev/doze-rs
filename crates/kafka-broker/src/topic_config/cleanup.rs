//! What a topic's `cleanup.policy` asks of the broker, and what compaction is told of the topic.

use std::collections::BTreeMap;

use super::{Defaults, effective};

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

/// Whether the topic's policy names `compact` (alone or with `delete`): its log is cleaned.
pub(crate) fn compacts(own: &BTreeMap<String, String>) -> bool {
    own.get("cleanup.policy").is_some_and(|p| {
        p.split(',')
            .any(|k| k.trim().eq_ignore_ascii_case("compact"))
    })
}

/// What compaction is asked of for this topic: its ratio, lags and how long a deletion is kept, as it is configured, and the time.
pub(crate) fn cleaner(
    defaults: &Defaults,
    own: &BTreeMap<String, String>,
    now_ms: i64,
    map_bytes: usize,
) -> kafka_storage::compact::Params {
    let all = effective(defaults, own);
    let number = |name: &str| {
        all.iter()
            .find(|e| e.name == name)
            .map(|e| e.value.as_str())
    };
    let int = |name: &str, fallback: i64| {
        number(name)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(fallback)
    };
    kafka_storage::compact::Params {
        min_dirty_ratio: number("min.cleanable.dirty.ratio")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.5),
        min_lag_ms: int("min.compaction.lag.ms", 0),
        max_lag_ms: int("max.compaction.lag.ms", i64::MAX),
        delete_retention_ms: int("delete.retention.ms", 86_400_000),
        now_ms,
        map_bytes,
    }
}
