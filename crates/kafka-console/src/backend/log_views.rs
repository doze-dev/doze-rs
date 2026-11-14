//! What the Log and Producers pages may read from the broker that the protocol does not say: a partition's log (its
//! segments, the planner's verdict, what the cleaner and retention last did) and the window of batches it keeps for each
//! producer. Each is checked against the console's right to describe (the log) or read (the producers) the topic.

use kafka_broker::{LogFacts, PartitionProducers};

use super::Backend;

impl Backend {
    /// The log of one partition, if the console may describe the topic and the partition exists.
    pub async fn log_facts(&self, topic: &str, partition: i32) -> Option<LogFacts> {
        self.broker
            .console_view()
            .log_facts(&self.caller, topic, partition)
            .await
    }

    /// The newest round of cleaning put in place on a topic the console may describe (topic, partition, round).
    pub fn last_cleaner_round(&self) -> Option<(String, i32, kafka_broker::RoundNote)> {
        self.broker.console_view().last_round(&self.caller)
    }

    /// The producers of one partition, if the console may read the topic and the partition exists.
    pub async fn partition_producers(
        &self,
        topic: &str,
        partition: i32,
    ) -> Option<PartitionProducers> {
        self.broker
            .console_view()
            .producers(&self.caller, topic, partition)
            .await
    }
}
