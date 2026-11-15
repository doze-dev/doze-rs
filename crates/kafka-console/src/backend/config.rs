//! What the Config pages may read from the broker that the protocol does not say: the rule for each topic config's value
//! and how often the broker looks at logs. Both are checked against the console's right to describe configs.

use kafka_broker::{ConfigRule, Pace};

use super::Backend;

impl Backend {
    /// The rule of every topic config for `topic`, if the console may describe its configs.
    #[must_use]
    pub fn topic_config_rules(&self, topic: &str) -> Vec<ConfigRule> {
        self.broker
            .console_view()
            .topic_config_rules(&self.caller, topic)
            .unwrap_or_default()
    }

    /// How often the cleaner and retention look at logs, if the console may describe the broker's configs.
    #[must_use]
    pub fn pace(&self) -> Option<Pace> {
        self.broker.console_view().pace(&self.caller)
    }
}
