//! What the topic forms may read from the broker that the protocol does not say: the partition limits and Kafka's own
//! table of topic configs. Both are checked against the console's right to create topics.

use kafka_broker::{ConfigHelp, Limits};

use super::Backend;

impl Backend {
    /// The partition limits (`--max-partitions`, `--default-partitions`), if the console may create topics.
    #[must_use]
    pub fn limits(&self) -> Option<Limits> {
        self.broker.console_view().limits(&self.caller)
    }

    /// Kafka's table of topic configs as a new topic has them, if the console may create topics.
    #[must_use]
    pub fn topic_config_help(&self) -> Option<Vec<ConfigHelp>> {
        self.broker
            .console_view()
            .topic_config_defaults(&self.caller)
    }
}
