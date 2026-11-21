//! What the broker's configuration takes from the flags by working it out, not by copying a value, and the numbers it
//! checks first.

use anyhow::{Result, bail};
use kafka_broker::Defaults;

use super::Args;

impl Args {
    /// The topic config defaults the flags set (a retention of 0 or less is none: -1).
    pub(super) fn topic_defaults(&self) -> Defaults {
        let none_is_zero = |n: i64| if n <= 0 { -1 } else { n };
        Defaults {
            segment_bytes: self.segment_bytes,
            index_interval_bytes: self.index_interval_bytes,
            message_max_bytes: self.message_max_bytes,
            retention_bytes: none_is_zero(self.retention_bytes),
            retention_ms: none_is_zero(self.retention_ms),
        }
    }

    /// The sizes, counts and times that must be above zero.
    pub(super) fn check_numbers(&self) -> Result<()> {
        for (name, value) in [
            ("--segment-bytes", self.segment_bytes),
            ("--index-interval-bytes", self.index_interval_bytes),
            ("--message-max-bytes", self.message_max_bytes),
        ] {
            if value <= 0 {
                bail!("{name} must be positive, not {value}");
            }
        }
        if self.default_partitions <= 0
            || self.max_partitions <= 0
            || self.default_partitions > self.max_partitions
        {
            bail!(
                "--default-partitions ({}) must be positive and at most --max-partitions ({})",
                self.default_partitions,
                self.max_partitions
            );
        }
        for (name, time) in [
            ("--consumer-session-timeout", self.consumer_session_timeout),
            ("--log-cleaner-interval", self.log_cleaner_interval),
            (
                "--log-retention-check-interval",
                self.log_retention_check_interval,
            ),
        ] {
            if time.is_zero() {
                bail!("{name} must be above zero");
            }
        }
        if self.worker_threads == 0 {
            bail!("--worker-threads must be at least 1");
        }
        Ok(())
    }
}
