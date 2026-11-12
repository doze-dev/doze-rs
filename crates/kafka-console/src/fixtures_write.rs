//! The "larder" world of the topic write boards as view-models, and a small stand-in for the broker's answers to the
//! writes (the same refusals the real broker gives, for the same reasons), so the example server and the board tests
//! answer a POST without a broker. Used by the tests and by the `boards` example (feature `examples`).

mod admin;
mod create;
mod produce;

pub use admin::{
    add_form, add_partitions, delete_form, delete_records, delete_records_form, delete_topic,
};
pub use create::{create_landing, create_topic, draft_board, inputs, refused_board};
pub use produce::{produce_form, produce_send};

use crate::fixtures::ADDRESS;

/// The time of the boards' world: Nov 14, 22:15 UTC.
pub const NOW_MS: i64 = 1_700_000_100_000;
/// When the record of the Produced board was written: 22:14:41.208.
pub const SENT_MS: i64 = 1_700_000_081_208;

/// A topic of the world.
#[derive(Debug, Clone, Copy)]
pub struct Topic {
    pub name: &'static str,
    pub partitions: i32,
    pub cleanup: &'static str,
    pub records: i64,
    /// Milliseconds since the last write; none: never.
    pub written_ago_ms: Option<i64>,
}

/// The nine topics and twenty-one partitions of `kit/world.md`.
pub const TOPICS: [Topic; 9] = [
    Topic {
        name: "orders",
        partitions: 3,
        cleanup: "delete",
        records: 48_213,
        written_ago_ms: Some(1_000),
    },
    Topic {
        name: "payments.settled",
        partitions: 4,
        cleanup: "delete",
        records: 12_601,
        written_ago_ms: Some(11_000),
    },
    Topic {
        name: "inventory.levels",
        partitions: 1,
        cleanup: "compact",
        records: 9_820,
        written_ago_ms: Some(3_000),
    },
    Topic {
        name: "customer.profiles",
        partitions: 1,
        cleanup: "compact",
        records: 4_402,
        written_ago_ms: Some(21_600_000),
    },
    Topic {
        name: "pick-lists",
        partitions: 1,
        cleanup: "delete",
        records: 2_310,
        written_ago_ms: Some(41_000),
    },
    Topic {
        name: "pick-lists.dlq",
        partitions: 1,
        cleanup: "delete",
        records: 3,
        written_ago_ms: Some(259_200_000),
    },
    Topic {
        name: "notifications.email",
        partitions: 3,
        cleanup: "delete",
        records: 7_200,
        written_ago_ms: Some(60_000),
    },
    Topic {
        name: "clickstream",
        partitions: 6,
        cleanup: "delete",
        records: 903_114,
        written_ago_ms: Some(200),
    },
    Topic {
        name: "orders.dlq",
        partitions: 1,
        cleanup: "delete",
        records: 0,
        written_ago_ms: None,
    },
];

/// The topic called `name`, if the world has it.
#[must_use]
pub fn topic(name: &str) -> Option<&'static Topic> {
    TOPICS.iter().find(|t| t.name == name)
}

/// The address clients use.
#[must_use]
pub fn address() -> String {
    ADDRESS.to_owned()
}
