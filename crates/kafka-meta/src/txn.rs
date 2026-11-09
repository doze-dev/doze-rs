//! A transactional id's record: who owns it, and where its transaction stands. Written before the broker acts on a
//! change, so that a stop in the middle of ending a transaction is finished on the next start. A version byte, then
//! postcard, as for topics.

use serde::{Deserialize, Serialize};

use crate::MetaError;

const VERSION: u8 = 1;

/// Where a transactional id's transaction stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxnState {
    /// No transaction in progress, and none left undecided.
    Empty,
    /// Partitions and offsets are being added to one.
    Ongoing,
    /// The decision is made (commit); markers are to be written.
    PrepareCommit,
    /// The decision is made (abort); markers are to be written.
    PrepareAbort,
    /// The markers are written and the offsets applied.
    CompleteCommit,
    /// The markers are written.
    CompleteAbort,
}

/// An offset a transaction commits for a consumer group, staged until the transaction ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxnOffset {
    pub group: String,
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub leader_epoch: i32,
    pub metadata: String,
}

/// What is recorded of one transactional id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxnRecord {
    pub id: String,
    pub producer_id: i64,
    pub epoch: i16,
    /// The epoch before the last bump (-1: none), so that a producer asking again with the epoch it had, because the
    /// answer was lost, is given the same answer and not fenced.
    pub last_epoch: i16,
    /// The producer id before the last rotation (an id whose epochs ran out gets a new producer id), -1 if none.
    pub prev_producer_id: i64,
    pub timeout_ms: i32,
    pub state: TxnState,
    /// When the transaction began, in ms since the epoch.
    pub started_ms: i64,
    /// When the record last changed: an id nobody has touched for a long time is forgotten from it.
    pub updated_ms: i64,
    /// The partitions the transaction writes to (topic, partition).
    pub partitions: Vec<(String, i32)>,
    /// The groups whose offsets it commits.
    pub groups: Vec<String>,
    pub offsets: Vec<TxnOffset>,
}

impl TxnRecord {
    /// A transactional id nobody has initialised a producer for.
    #[must_use]
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            producer_id: -1,
            epoch: -1,
            last_epoch: -1,
            prev_producer_id: -1,
            timeout_ms: 0,
            state: TxnState::Empty,
            started_ms: 0,
            updated_ms: 0,
            partitions: Vec::new(),
            groups: Vec::new(),
            offsets: Vec::new(),
        }
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, MetaError> {
        postcard::to_extend(self, vec![VERSION])
            .map_err(|e| MetaError::Corrupt(format!("a transaction record cannot be written: {e}")))
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, MetaError> {
        let corrupt = |why: &str| MetaError::Corrupt(format!("transaction record: {why}"));
        let (version, body) = bytes.split_first().ok_or_else(|| corrupt("empty"))?;
        if *version != VERSION {
            return Err(corrupt(&format!("version {version} is not known")));
        }
        postcard::from_bytes(body).map_err(|e| corrupt(&e.to_string()))
    }
}

impl TxnState {
    /// Whether the transaction has not finished: its offsets, if any, are neither committed nor gone.
    #[must_use]
    pub fn is_open(self) -> bool {
        matches!(
            self,
            Self::Ongoing | Self::PrepareCommit | Self::PrepareAbort
        )
    }

    /// Kafka's name for the state, as DescribeTransactions and ListTransactions show it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Empty => "Empty",
            Self::Ongoing => "Ongoing",
            Self::PrepareCommit => "PrepareCommit",
            Self::PrepareAbort => "PrepareAbort",
            Self::CompleteCommit => "CompleteCommit",
            Self::CompleteAbort => "CompleteAbort",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_is_read_back_whole_and_an_unknown_version_is_refused() {
        let mut r = TxnRecord::new("tx");
        r.producer_id = 1000;
        r.epoch = 3;
        r.state = TxnState::PrepareCommit;
        r.partitions = vec![("t".into(), 0), ("u".into(), 2)];
        r.groups = vec!["g".into()];
        r.offsets = vec![TxnOffset {
            group: "g".into(),
            topic: "t".into(),
            partition: 0,
            offset: 5,
            leader_epoch: -1,
            metadata: "m".into(),
        }];
        let mut bytes = r.encode().unwrap();
        assert_eq!(TxnRecord::decode(&bytes).unwrap(), r);
        bytes[0] = 9;
        assert!(TxnRecord::decode(&bytes).is_err());
        assert!(TxnRecord::decode(&[]).is_err());
    }
}
