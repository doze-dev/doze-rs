//! How a topic is written to disk: a version byte, then postcard. The bytes are ours and not redb's, so a
//! change of either library cannot change what is stored, and a record of a version this build does not know
//! is refused rather than misread.

use std::collections::BTreeMap;

use kafka_wire::ids::{PartitionCount, TopicId, TopicName};
use serde::{Deserialize, Serialize};

use crate::MetaError;

const VERSION: u8 = 1;

/// A topic as the broker knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topic {
    /// Its name.
    pub name: TopicName,
    /// Its identity, new for every topic that is created.
    pub id: TopicId,
    /// How many partitions it has.
    pub partitions: PartitionCount,
    /// The configs set on it; an absent key has the broker's default.
    pub config: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    id: [u8; 16],
    partitions: i32,
    config: BTreeMap<String, String>,
}

impl Topic {
    pub(crate) fn encode(&self) -> Result<Vec<u8>, MetaError> {
        let stored = Stored {
            id: *self.id.as_bytes(),
            partitions: self.partitions.get(),
            config: self.config.clone(),
        };
        postcard::to_extend(&stored, vec![VERSION]).map_err(|e| {
            MetaError::Corrupt(format!("topic '{}' cannot be written: {e}", self.name))
        })
    }

    pub(crate) fn decode(name: &str, bytes: &[u8]) -> Result<Self, MetaError> {
        let corrupt = |why: &str| MetaError::Corrupt(format!("topic '{name}': {why}"));
        let (version, body) = bytes.split_first().ok_or_else(|| corrupt("empty"))?;
        if *version != VERSION {
            return Err(corrupt(&format!("version {version} is not known")));
        }
        let stored: Stored = postcard::from_bytes(body).map_err(|e| corrupt(&e.to_string()))?;
        Ok(Self {
            name: TopicName::parse(name).map_err(|e| corrupt(&e.to_string()))?,
            id: TopicId::from_bytes(stored.id),
            partitions: PartitionCount::new(stored.partitions)
                .ok_or_else(|| corrupt("no partitions"))?,
            config: stored.config,
        })
    }
}
