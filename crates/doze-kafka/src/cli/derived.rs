//! What the broker's configuration takes from the flags by working it out, not by copying a value.

use kafka_broker::{Defaults, TxnVersion};
use kafka_wire::profile::{KafkaMajor, Profile};

use super::Args;

/// How the broker ends a transaction on its own at a Kafka major: the `transaction.version` its profile finalizes.
pub(super) fn transaction_version(major: KafkaMajor) -> TxnVersion {
    if Profile::new(major).transaction_version() >= 2 {
        TxnVersion::EpochBump
    } else {
        TxnVersion::Classic
    }
}

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
}
