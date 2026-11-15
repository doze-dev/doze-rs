//! The user in control of their data (docs/port/data-control.md): what is using the disk and why, and three ways to give it
//! back.
//!
//! - [`Broker::data_usage`]: the data directory by kind (topics, the metadata store, anything else), each topic's bytes and
//!   what its configs keep, and the room left on the volume. Read from the files on request; nothing is kept.
//! - [`Broker::empty_topic`] and [`Broker::empty_topics`]: every record of a topic gone and its disk freed now, the topic,
//!   its partitions, its configs and the groups' committed offsets kept. Unlike DeleteRecords this is the broker's own
//!   action, so it empties a compacted topic too, and it takes the active segment with the rest. Each partition carries on
//!   from its end: an offset is never handed out twice. Refused while a transaction is under way on the topic.
//! - [`Broker::erase_everything`]: every topic, every group and its committed offsets, every finished transactional id.
//!   ACLs, users and the broker's dynamic configs stay. Refused while any transaction is under way.
//!
//! Each checks the right Kafka would ask for the nearest call (DELETE on the topic; ALTER on the cluster for the ones that
//! reach every topic), so a caller cannot do here what its ACLs refuse over the protocol.

mod empty;
mod erase;
mod usage;
mod words;

pub use empty::{Emptied, EmptiedAll};
pub use erase::Erased;
pub use usage::{DataUsage, Keeps, Stray, TopicUsage};
pub use words::{bytes_words, duration_words};

use kafka_wire::ResponseError;

/// Why a data action did nothing, or stopped part-way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataRefusal {
    /// The caller may not: the right asked for, as Kafka names it (`DELETE on topic orders`).
    Denied { right: String, cluster: bool },
    /// There is no such topic.
    NoTopic { topic: String },
    /// A transaction is under way on the topic (or, for Erase everything, anywhere); `id` is its transactional id when the
    /// coordinator knows it.
    TxnOpen {
        topic: Option<String>,
        id: Option<String>,
    },
    /// Something could not be done on disk (`what`: `partition 2 of orders`, `the topic orders`): what was done before it
    /// stays done.
    Storage { what: String, message: String },
}

impl DataRefusal {
    /// The Kafka error that names this refusal.
    #[must_use]
    pub fn error(&self) -> ResponseError {
        match self {
            Self::Denied { cluster: true, .. } => ResponseError::ClusterAuthorizationFailed,
            Self::Denied { .. } => ResponseError::TopicAuthorizationFailed,
            Self::NoTopic { .. } => ResponseError::UnknownTopicOrPartition,
            Self::TxnOpen { .. } => ResponseError::ConcurrentTransactions,
            Self::Storage { .. } => ResponseError::KafkaStorageError,
        }
    }

    /// What happened and what to do, in a sentence.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Self::Denied { right, .. } => {
                format!("This needs {right}, which the ACLs do not give. Nothing was changed.")
            }
            Self::NoTopic { topic } => format!("There is no topic called {topic}."),
            Self::TxnOpen { topic, id } => {
                let whose = id
                    .as_deref()
                    .map_or_else(String::new, |id| format!(" (transactional id {id})"));
                let on = topic
                    .as_deref()
                    .map_or_else(|| "on this broker".to_owned(), |t| format!("on {t}"));
                format!(
                    "A transaction is under way {on}{whose}. Emptying now would cut it in half, so nothing was changed: commit or abort it (or wait for its timeout), then try again."
                )
            }
            Self::Storage { what, message } => format!(
                "Doing it to {what} failed on disk: {message}. What was done before it stays done; the rest was not touched."
            ),
        }
    }
}

#[cfg(test)]
mod tests;
