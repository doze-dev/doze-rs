//! The Lab's three rehearsals that are not rules, asked of the broker as the console's principal: a member's session runs out, a
//! group rebalances, a compacted partition is cleaned now. The broker checks the right and takes the step its own machinery takes.

use kafka_broker::{CleanOutcome, LabOutcome};

use super::Backend;

impl Backend {
    /// Runs out the session of one member of a group now.
    pub async fn lab_expire(&self, group: &str, member: &str) -> LabOutcome {
        self.broker
            .console_lab()
            .expire_member(&self.caller, group, member)
            .await
    }

    /// Starts a rebalance of a group now.
    pub async fn lab_rebalance(&self, group: &str) -> LabOutcome {
        self.broker
            .console_lab()
            .force_rebalance(&self.caller, group)
            .await
    }

    /// Runs the cleaner on one partition now, if the planner finds a round due.
    pub async fn lab_clean(&self, topic: &str, partition: i32) -> CleanOutcome {
        self.broker
            .console_lab()
            .clean_now(&self.caller, topic, partition)
            .await
    }
}
