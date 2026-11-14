//! What a topic config's value may be, in words, for the Config page of the console: the table's own rule for each key,
//! so the page never carries a second copy of it.

use kafka_auth::authz::operation;
use kafka_storage::fs::Fs;

use super::ConsoleView;
use crate::Caller;
use crate::topic_config;

/// One topic config's rule for a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigRule {
    /// The key (`retention.ms`).
    pub name: &'static str,
    /// What a value may be, in words (`a number from 0 to 1`).
    pub allowed: String,
}

impl<F: Fs> ConsoleView<'_, F> {
    /// The rule of every topic config in the order Kafka lists them, if `caller` may DESCRIBE_CONFIGS of the topic and
    /// it exists; `None` otherwise (the two are not told apart, as Kafka does not).
    #[must_use]
    pub fn topic_config_rules(&self, caller: &Caller, topic: &str) -> Option<Vec<ConfigRule>> {
        let b = self.broker;
        if !b.topic_allows(caller, topic, operation::DESCRIBE_CONFIGS) {
            return None;
        }
        let name = kafka_wire::ids::TopicName::parse(topic).ok()?;
        b.meta.topic(&name)?;
        let entries =
            topic_config::effective(&b.defaults.get(), &std::collections::BTreeMap::new());
        Some(
            entries
                .into_iter()
                .filter_map(|e| {
                    Some(ConfigRule {
                        name: e.name,
                        allowed: topic_config::allowed_of(e.name)?,
                    })
                })
                .collect(),
        )
    }
}
