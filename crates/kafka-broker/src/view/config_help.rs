//! What a new topic starts with, as Kafka's own table says it: every topic config the broker keeps, its default and the
//! sentence Kafka gives for it. The Create-topic form teaches from this, so it never carries a second copy of the table.

use std::collections::BTreeMap;

use kafka_auth::authz::operation;
use kafka_storage::fs::Fs;

use super::ConsoleView;
use crate::Caller;
use crate::topic_config;

/// One topic config as a topic that sets nothing has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigHelp {
    /// The key (`retention.ms`).
    pub name: &'static str,
    /// The value when the topic sets nothing: Kafka's default, or this broker's own setting.
    pub default: String,
    /// What Kafka says it is for (DescribeConfigs documentation; it may carry HTML tags, which a page must not render).
    pub doc: &'static str,
}

/// The partition limits the Create-topic and Add-partitions forms show beside their fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most partitions one topic may have (`--max-partitions`).
    pub max_partitions: i32,
    /// How many partitions a topic made without a count gets (`--default-partitions`).
    pub default_partitions: i32,
}

impl<F: Fs> ConsoleView<'_, F> {
    /// The partition limits, if `caller` may create topics (what the forms that show them are for).
    #[must_use]
    pub fn limits(&self, caller: &Caller) -> Option<Limits> {
        let b = self.broker;
        b.cluster_allows(caller, operation::CREATE)
            .then_some(Limits {
                max_partitions: b.config.max_partitions,
                default_partitions: b.config.default_partitions,
            })
    }

    /// The topic configs of a topic that sets none, in the order Kafka lists them, if `caller` may create topics (the
    /// Create-topic form is what asks). `None` otherwise.
    #[must_use]
    pub fn topic_config_defaults(&self, caller: &Caller) -> Option<Vec<ConfigHelp>> {
        let b = self.broker;
        if !b.cluster_allows(caller, operation::CREATE) {
            return None;
        }
        let entries = topic_config::effective(&b.defaults.get(), &BTreeMap::new());
        Some(
            entries
                .into_iter()
                .map(|e| ConfigHelp {
                    name: e.name,
                    default: e.value,
                    doc: e.doc,
                })
                .collect(),
        )
    }
}
