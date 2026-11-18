//! What a share group keeps of one partition (KIP-932): where its share-partition starts and the state of each record
//! past that which is not simply waiting to be delivered. It is Kafka's share-coordinator record
//! (`ShareSnapshotValue`: state epoch, start offset, state batches) kept here instead of in `__share_group_state`. A
//! version byte, then postcard, as for topics.

use serde::{Deserialize, Serialize};

use crate::MetaError;

const VERSION: u8 = 1;

/// The durable state of a record, as Kafka's share coordinator stores it. A record that a consumer holds (acquired) is
/// stored as available: a lock does not outlive the broker that granted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShareRecordState {
    /// To be delivered (again, if its delivery count is not 0).
    Available,
    /// A consumer accepted it.
    Acknowledged,
    /// Rejected, or delivered as often as the group allows, or before the start: never delivered again.
    Archived,
}

/// A run of records with the same state and delivery count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareBatch {
    /// The first offset.
    pub first: i64,
    /// The last offset (inclusive).
    pub last: i64,
    /// Their state.
    pub state: ShareRecordState,
    /// How many times they have been delivered.
    pub delivery_count: i16,
}

/// What a share group keeps of one partition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareState {
    /// Bumped each time the state is set from outside (AlterShareGroupOffsets): Kafka's state epoch.
    pub state_epoch: i32,
    /// The share-partition start offset (SPSO): every record before it is done with.
    pub start_offset: i64,
    /// The records at or past the start whose state is not "available, never delivered", in order, not overlapping.
    pub batches: Vec<ShareBatch>,
}

impl ShareState {
    pub(crate) fn encode(&self) -> Result<Vec<u8>, MetaError> {
        postcard::to_extend(self, vec![VERSION])
            .map_err(|e| MetaError::Corrupt(format!("a share state cannot be written: {e}")))
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, MetaError> {
        let corrupt = |why: &str| MetaError::Corrupt(format!("share state: {why}"));
        let (version, body) = bytes.split_first().ok_or_else(|| corrupt("empty"))?;
        match *version {
            VERSION => postcard::from_bytes(body).map_err(|e| corrupt(&e.to_string())),
            _ => Err(corrupt(&format!("version {version} is not known"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_is_read_back_whole_and_an_unknown_version_is_refused() {
        let state = ShareState {
            state_epoch: 2,
            start_offset: 10,
            batches: vec![
                ShareBatch {
                    first: 10,
                    last: 12,
                    state: ShareRecordState::Acknowledged,
                    delivery_count: 1,
                },
                ShareBatch {
                    first: 15,
                    last: 15,
                    state: ShareRecordState::Archived,
                    delivery_count: 5,
                },
            ],
        };
        let mut bytes = state.encode().unwrap();
        assert_eq!(ShareState::decode(&bytes).unwrap(), state);
        bytes[0] = 9;
        assert!(ShareState::decode(&bytes).is_err());
        assert!(ShareState::decode(&[]).is_err());
    }
}
