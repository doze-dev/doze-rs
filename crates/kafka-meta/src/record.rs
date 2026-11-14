//! How a topic is written to disk: a version byte, then postcard. The bytes are ours and not redb's, so a
//! change of either library cannot change what is stored, and a record of a version this build does not know
//! is refused rather than misread.

use std::collections::BTreeMap;

use kafka_wire::ids::{PartitionCount, TopicId, TopicName};
use serde::{Deserialize, Serialize};

use crate::MetaError;
use crate::settings::TopicSettings;

const VERSION: u8 = 1;

/// A topic as the broker knows it. Made with [`Topic::new`], which reads its settings from its configs; a topic is shared
/// behind an `Arc` and replaced, never changed, when its configs change.
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
    /// What the broker acts on of `config` as it stores a batch.
    settings: TopicSettings,
}

/// What is written of a topic (its name is the key it is stored under).
#[derive(Serialize)]
struct Stored<'a> {
    id: [u8; 16],
    partitions: i32,
    config: &'a BTreeMap<String, String>,
}

/// What is read back.
#[derive(Deserialize)]
struct Read {
    id: [u8; 16],
    partitions: i32,
    config: BTreeMap<String, String>,
}

impl Topic {
    /// A topic with these configs set on it.
    #[must_use]
    pub fn new(
        name: TopicName,
        id: TopicId,
        partitions: PartitionCount,
        config: BTreeMap<String, String>,
    ) -> Self {
        let settings = TopicSettings::of(&config);
        Self {
            name,
            id,
            partitions,
            config,
            settings,
        }
    }

    /// This topic with `config` set on it in place of its configs.
    #[must_use]
    pub fn with_config(&self, config: BTreeMap<String, String>) -> Self {
        Self::new(self.name.clone(), self.id, self.partitions, config)
    }

    /// This topic grown to `partitions`.
    #[must_use]
    pub fn with_partitions(&self, partitions: PartitionCount) -> Self {
        Self {
            partitions,
            ..self.clone()
        }
    }

    /// What the broker acts on of its configs as it stores a batch.
    #[must_use]
    pub fn settings(&self) -> &TopicSettings {
        &self.settings
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, MetaError> {
        let stored = Stored {
            id: *self.id.as_bytes(),
            partitions: self.partitions.get(),
            config: &self.config,
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
        let stored: Read = postcard::from_bytes(body).map_err(|e| corrupt(&e.to_string()))?;
        Ok(Self::new(
            TopicName::parse(name).map_err(|e| corrupt(&e.to_string()))?,
            TopicId::from_bytes(stored.id),
            PartitionCount::new(stored.partitions).ok_or_else(|| corrupt("no partitions"))?,
            stored.config,
        ))
    }
}
