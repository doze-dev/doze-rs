//! A transactional id's record: who owns it, and where its transaction stands. Written before the broker acts on a
//! change, so that a stop in the middle of ending a transaction is finished on the next start. A version byte, then
//! postcard, as for topics.
//!
//! Version 2 adds what Kafka's `TransactionLogValue` v1 added for KIP-890: the transaction version the client used at
//! the last change, and the producer id the transaction hands over to when it completes (its epochs ran out).

use serde::{Deserialize, Serialize};

use crate::MetaError;

const VERSION: u8 = 2;

/// Which transaction protocol a request spoke (Kafka's `TransactionVersion`, the `transaction.version` feature).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxnVersion {
    /// The classic protocol (`TV_0`): partitions are added by AddPartitionsToTxn, and an end keeps the epoch.
    #[default]
    Classic,
    /// KIP-890's second part (`TV_2`): a write adds its partition, and every end raises the epoch.
    EpochBump,
}

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
    /// The transaction version of the request that made the last change (Kafka's `ClientTransactionVersion`).
    pub client_version: TxnVersion,
    /// The producer id a decided transaction hands over to when it completes, because its epochs ran out (-1: none;
    /// Kafka's `NextProducerId`). Chosen and stored with the decision, so a stop before the end changes nothing.
    pub next_producer_id: i64,
}

/// A record as the first version wrote it, before KIP-890's second part.
#[derive(Deserialize)]
struct V1 {
    id: String,
    producer_id: i64,
    epoch: i16,
    last_epoch: i16,
    prev_producer_id: i64,
    timeout_ms: i32,
    state: TxnState,
    started_ms: i64,
    updated_ms: i64,
    partitions: Vec<(String, i32)>,
    groups: Vec<String>,
    offsets: Vec<TxnOffset>,
}

impl From<V1> for TxnRecord {
    fn from(old: V1) -> Self {
        Self {
            id: old.id,
            producer_id: old.producer_id,
            epoch: old.epoch,
            last_epoch: old.last_epoch,
            prev_producer_id: old.prev_producer_id,
            timeout_ms: old.timeout_ms,
            state: old.state,
            started_ms: old.started_ms,
            updated_ms: old.updated_ms,
            partitions: old.partitions,
            groups: old.groups,
            offsets: old.offsets,
            client_version: TxnVersion::Classic,
            next_producer_id: -1,
        }
    }
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
            client_version: TxnVersion::Classic,
            next_producer_id: -1,
        }
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, MetaError> {
        postcard::to_extend(self, vec![VERSION])
            .map_err(|e| MetaError::Corrupt(format!("a transaction record cannot be written: {e}")))
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, MetaError> {
        let corrupt = |why: &str| MetaError::Corrupt(format!("transaction record: {why}"));
        let (version, body) = bytes.split_first().ok_or_else(|| corrupt("empty"))?;
        match *version {
            VERSION => postcard::from_bytes(body).map_err(|e| corrupt(&e.to_string())),
            1 => postcard::from_bytes::<V1>(body)
                .map(Self::from)
                .map_err(|e| corrupt(&e.to_string())),
            _ => Err(corrupt(&format!("version {version} is not known"))),
        }
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
        r.client_version = TxnVersion::EpochBump;
        r.next_producer_id = 2000;
        let mut bytes = r.encode().unwrap();
        assert_eq!(TxnRecord::decode(&bytes).unwrap(), r);
        bytes[0] = 9;
        assert!(TxnRecord::decode(&bytes).is_err());
        assert!(TxnRecord::decode(&[]).is_err());
    }

    /// The fields of the first version, in its order: what a broker of before KIP-890's second part wrote.
    #[derive(Serialize)]
    struct WrittenByV1<'a> {
        id: &'a str,
        producer_id: i64,
        epoch: i16,
        last_epoch: i16,
        prev_producer_id: i64,
        timeout_ms: i32,
        state: TxnState,
        started_ms: i64,
        updated_ms: i64,
        partitions: Vec<(String, i32)>,
        groups: Vec<String>,
        offsets: Vec<TxnOffset>,
    }

    #[test]
    fn a_record_of_the_first_version_is_read_as_a_classic_one_with_no_producer_id_to_hand_over() {
        let old = WrittenByV1 {
            id: "tx",
            producer_id: 1000,
            epoch: 4,
            last_epoch: 3,
            prev_producer_id: 999,
            timeout_ms: 60_000,
            state: TxnState::PrepareAbort,
            started_ms: 5,
            updated_ms: 6,
            partitions: vec![("t".into(), 1)],
            groups: vec!["g".into()],
            offsets: Vec::new(),
        };
        let bytes = postcard::to_extend(&old, vec![1]).unwrap();
        let read = TxnRecord::decode(&bytes).unwrap();
        let mut want = TxnRecord::new("tx");
        want.producer_id = 1000;
        want.epoch = 4;
        want.last_epoch = 3;
        want.prev_producer_id = 999;
        want.timeout_ms = 60_000;
        want.state = TxnState::PrepareAbort;
        want.started_ms = 5;
        want.updated_ms = 6;
        want.partitions = vec![("t".into(), 1)];
        want.groups = vec!["g".into()];
        assert_eq!(read, want);
        assert_eq!(
            (read.client_version, read.next_producer_id),
            (TxnVersion::Classic, -1)
        );
    }
}
