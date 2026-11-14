//! How often the broker looks at logs to clean or trim them: what "a change counts at the next look" means in seconds.

use std::time::Duration;

use kafka_auth::authz::operation;
use kafka_storage::fs::Fs;

use super::ConsoleView;
use crate::Caller;

/// The pace of the background passes that read a topic's configs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pace {
    /// How often compacted topics are looked at (`--log-cleaner-interval`).
    pub cleaner_every: Duration,
    /// Whether compacted topics are cleaned at all (`--log-cleaner-enable`).
    pub cleaner_on: bool,
    /// How often every topic's retention is applied (`log.retention.check.interval.ms`).
    pub retention_every: Duration,
}

impl<F: Fs> ConsoleView<'_, F> {
    /// The pace of the cleaner and of retention, if `caller` may DESCRIBE_CONFIGS of the cluster (what the broker's own
    /// configs are listed under); `None` otherwise.
    #[must_use]
    pub fn pace(&self, caller: &Caller) -> Option<Pace> {
        let b = self.broker;
        b.cluster_allows(caller, operation::DESCRIBE_CONFIGS)
            .then_some(Pace {
                cleaner_every: b.config.log_cleaner_every,
                cleaner_on: b.config.log_cleaner_enable,
                retention_every: b.config.log_retention_check_every,
            })
    }
}
