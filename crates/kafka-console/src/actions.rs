//! The writes the console may issue: the one list. A page that changes the broker is a form that names its API
//! (`data-op`), and the name must be one of these; a test compares this list with the `data-op` names the design boards
//! draw, both ways, so the console issues nothing a board does not show and shows no write that is not here.

/// A Kafka API the console writes with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    /// Slice 9b: make a topic (the Create-topic form; "Check only" is the same call with `validate_only`).
    CreateTopics,
    /// Slice 9b: write one record.
    Produce,
    /// Slice 9b: raise a topic's partition count.
    CreatePartitions,
    /// Slice 9b: move a partition's log start up.
    DeleteRecords,
    /// Slice 9b: delete a topic.
    DeleteTopics,
}

impl Action {
    /// Every write of the console.
    pub const ALL: [Self; 5] = [
        Self::CreateTopics,
        Self::Produce,
        Self::CreatePartitions,
        Self::DeleteRecords,
        Self::DeleteTopics,
    ];

    /// The name a form carries in `data-op`: Kafka's own name for the API.
    #[must_use]
    pub const fn op(self) -> &'static str {
        match self {
            Self::CreateTopics => "CreateTopics",
            Self::Produce => "Produce",
            Self::CreatePartitions => "CreatePartitions",
            Self::DeleteRecords => "DeleteRecords",
            Self::DeleteTopics => "DeleteTopics",
        }
    }

    /// The action named `op`, if it is one.
    #[must_use]
    pub fn named(op: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.op() == op)
    }
}

/// The boards of the slices that write, by id: the ones whose `data-op` names the test compares with [`Action::ALL`].
pub const WRITE_BOARDS: [&str; 14] = [
    "topic-create",
    "topic-create-config",
    "topic-create-review",
    "topic-create-refused",
    "topic-created",
    "topic-produce",
    "topic-produce-keyless",
    "topic-produced",
    "topic-add-partitions",
    "topic-add-partitions-refused",
    "topic-delete-records",
    "topic-delete-records-refused",
    "topic-delete",
    "topic-deleted",
];

#[cfg(test)]
mod tests;
