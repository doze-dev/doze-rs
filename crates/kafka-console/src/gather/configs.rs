//! The few configs the topic pages say something about, from DescribeConfigs.

use std::collections::HashMap;

use kafka_wire::api::Response;

use crate::backend::{Backend, requests};
use crate::model::Cleanup;

/// The config source of a value set on the topic itself.
const DYNAMIC_TOPIC: i8 = 1;
/// Kafka's default for `retention.ms`: seven days.
const DEFAULT_RETENTION_MS: i64 = 604_800_000;

/// What the pages say of a topic's configs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TopicConfigs {
    pub cleanup: Cleanup,
    /// `retention.ms` in force (`-1` is for ever).
    pub retention_ms: i64,
    /// The topic sets its own retention (the list says so; the default is not worth a line).
    pub retention_set: bool,
}

impl Default for TopicConfigs {
    fn default() -> Self {
        Self {
            cleanup: Cleanup::Delete,
            retention_ms: DEFAULT_RETENTION_MS,
            retention_set: false,
        }
    }
}

/// `cleanup.policy` as the topic pages group it.
#[must_use]
pub(super) fn cleanup_of(policy: &str) -> Cleanup {
    let compact = policy.split(',').any(|p| p.trim() == "compact");
    let delete = policy.split(',').any(|p| p.trim() == "delete");
    match (compact, delete) {
        (true, true) => Cleanup::Both,
        (true, false) => Cleanup::Compact,
        _ => Cleanup::Delete,
    }
}

/// The configs of these topics (at most 200), by name. A topic the console may not describe is left out.
pub(super) async fn of(b: &Backend, names: &[String]) -> HashMap<String, TopicConfigs> {
    let Ok(Response::DescribeConfigs(r)) = b.call(requests::describe_topic_configs(names)).await
    else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for res in r.results.iter().filter(|r| r.error_code == 0) {
        let mut c = TopicConfigs::default();
        for cfg in &res.configs {
            let value = cfg.value.as_ref().map(ToString::to_string);
            match (cfg.name.as_str(), value) {
                ("cleanup.policy", Some(v)) => c.cleanup = cleanup_of(&v),
                ("retention.ms", Some(v)) => {
                    if let Ok(ms) = v.parse::<i64>() {
                        c.retention_ms = ms;
                        c.retention_set = cfg.config_source == DYNAMIC_TOPIC;
                    }
                }
                _ => {}
            }
        }
        out.insert(res.resource_name.to_string(), c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_policy_is_delete_compact_or_both() {
        for (text, want) in [
            ("delete", Cleanup::Delete),
            ("compact", Cleanup::Compact),
            ("compact,delete", Cleanup::Both),
            ("delete, compact", Cleanup::Both),
            ("", Cleanup::Delete),
            ("compaction", Cleanup::Delete),
        ] {
            assert_eq!(cleanup_of(text), want, "{text:?}");
        }
    }
}
