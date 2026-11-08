//! Committed offsets as the store keeps them: what a group has consumed up to, in each partition it has committed.
//! A version byte, then postcard, as for topics.

use kafka_wire::ids::{PartitionId, TopicName};
use serde::{Deserialize, Serialize};

use crate::MetaError;

const VERSION: u8 = 1;

/// One committed offset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredOffset {
    /// The next offset the group will read. (A client may commit any number, a negative one too; Kafka stores it.)
    pub offset: i64,
    /// The leader epoch the client saw there, or -1.
    pub leader_epoch: i32,
    /// The text the client attached.
    pub metadata: String,
    /// When it was committed, in milliseconds since the epoch.
    pub committed_ms: i64,
}

impl StoredOffset {
    pub(crate) fn encode(&self) -> Result<Vec<u8>, MetaError> {
        postcard::to_extend(self, vec![VERSION])
            .map_err(|e| MetaError::Corrupt(format!("an offset cannot be written: {e}")))
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, MetaError> {
        let corrupt = |why: &str| MetaError::Corrupt(format!("committed offset: {why}"));
        let (version, body) = bytes.split_first().ok_or_else(|| corrupt("empty"))?;
        if *version != VERSION {
            return Err(corrupt(&format!("version {version} is not known")));
        }
        postcard::from_bytes(body).map_err(|e| corrupt(&e.to_string()))
    }
}

/// An offset to commit, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetCommit {
    /// The topic; it must exist.
    pub topic: TopicName,
    /// The partition; it must exist.
    pub partition: PartitionId,
    /// What to store.
    pub offset: StoredOffset,
}

/// What became of one commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Committed {
    /// It is stored, and durable.
    Stored,
    /// There is no such topic or partition, so nothing was stored.
    UnknownTopicOrPartition,
}

/// An offset a group has committed, with its place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupOffset {
    /// The topic.
    pub topic: TopicName,
    /// The partition.
    pub partition: PartitionId,
    /// The offset.
    pub offset: StoredOffset,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_of_a_version_this_build_does_not_know_is_refused_not_misread() {
        let stored = StoredOffset {
            offset: -5,
            leader_epoch: -1,
            metadata: "m".into(),
            committed_ms: 7,
        };
        let mut bytes = stored.encode().unwrap();
        assert_eq!(StoredOffset::decode(&bytes).unwrap(), stored);
        bytes[0] = 2;
        assert!(StoredOffset::decode(&bytes).is_err());
        assert!(StoredOffset::decode(&[]).is_err());
    }
}
