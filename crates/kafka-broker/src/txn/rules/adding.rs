//! What a transaction may take: a write to a partition (KIP-890's first part refuses one the transaction does not have),
//! and partitions and groups added to it, by AddPartitionsToTxn and AddOffsetsToTxn or (the second part) by the writes and
//! offset commits themselves.

use kafka_meta::{TxnRecord, TxnState, TxnVersion};
use kafka_wire::ResponseError as E;

use super::{begin, can_add, owns};

/// Whether a batch from `producer` at `epoch` to this partition, in a Produce of `version`, is part of a transaction that
/// may take it: the producer owns the id, a transaction is open and the partition was added to it. Otherwise the batch
/// would sit in the log with no marker ever coming for it, holding the last stable offset still for ever.
///
/// The refusals are Kafka's for an unverified write (KIP-890's first part, `AddPartitionsToTxnManager` and
/// `ReplicaManager.handleProduceAppend`): a partition not in the transaction is INVALID_TXN_STATE up to v10 and
/// TRANSACTION_ABORTABLE from v11, which a client may abort and go on from; a transaction being ended is
/// CONCURRENT_TRANSACTIONS from v12 and NOT_ENOUGH_REPLICAS (retriable for any client) before.
pub(crate) fn may_write(
    rec: &TxnRecord,
    producer: i64,
    epoch: i16,
    at: (&str, i32),
    version: i16,
) -> Result<(), E> {
    owns(rec, producer, epoch).map_or(Ok(()), |e| Err(on_partition(e)))?;
    match rec.state {
        TxnState::PrepareCommit | TxnState::PrepareAbort => Err(if version >= 12 {
            E::ConcurrentTransactions
        } else {
            E::NotEnoughReplicas
        }),
        TxnState::Ongoing if holds(rec, at) => Ok(()),
        _ => Err(not_added(version, 11)),
    }
}

/// Whether the transaction has the partition.
pub(crate) fn holds(rec: &TxnRecord, (topic, partition): (&str, i32)) -> bool {
    rec.partitions
        .iter()
        .any(|(t, p)| t == topic && *p == partition)
}

/// What a write to a partition (or an offset commit for a group) not added to the transaction is told: INVALID_TXN_STATE,
/// or TRANSACTION_ABORTABLE from `abortable`, the version that knows it (Produce v11, TxnOffsetCommit v4).
pub(crate) fn not_added(version: i16, abortable: i16) -> E {
    if version >= abortable {
        E::TransactionAbortable
    } else {
        E::InvalidTxnState
    }
}

/// What a partition reports for the coordinator's refusal of its producer: PRODUCER_FENCED is INVALID_PRODUCER_EPOCH
/// there, which the client treats alike (`AddPartitionsToTxnManager`: "Producers expect to handle INVALID_PRODUCER_EPOCH").
pub(crate) fn on_partition(code: E) -> E {
    if code == E::ProducerFenced {
        E::InvalidProducerEpoch
    } else {
        code
    }
}

/// What a request to add to the transaction comes to, before anything is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Adding {
    /// Refused with this code.
    Refused(E),
    /// Everything asked for is in the open transaction already: success, and nothing to store.
    There,
    /// The record is to change ([`add`]) and be stored.
    Store,
}

/// The coordinator's answer to a request to add to the transaction (`handleAddPartitionsToTransaction`): the producer must
/// own the id and the transaction must not be being ended. `there` says whether everything asked for is in it already.
pub(crate) fn may_add(rec: &TxnRecord, producer: i64, epoch: i16, there: bool) -> Adding {
    match owns(rec, producer, epoch).or_else(|| can_add(rec)) {
        Some(code) => Adding::Refused(code),
        None if rec.state == TxnState::Ongoing && there => Adding::There,
        None => Adding::Store,
    }
}

/// Adds partitions to the transaction, beginning it if none is open, at the transaction version of the request that
/// added them (`TransactionMetadata.prepareAddPartitions`).
pub(crate) fn add(
    rec: &mut TxnRecord,
    partitions: impl IntoIterator<Item = (String, i32)>,
    version: TxnVersion,
    now_ms: i64,
) {
    begin(rec, now_ms);
    rec.client_version = version;
    for p in partitions {
        if !rec.partitions.contains(&p) {
            rec.partitions.push(p);
        }
    }
}

/// Adds a group whose offsets the transaction commits (Kafka adds the group's offsets partition), as [`add`] does.
pub(crate) fn add_group(rec: &mut TxnRecord, group: &str, version: TxnVersion, now_ms: i64) {
    begin(rec, now_ms);
    rec.client_version = version;
    if !has_group(rec, group) {
        rec.groups.push(group.to_owned());
    }
}

/// Whether the transaction commits offsets for the group.
pub(crate) fn has_group(rec: &TxnRecord, group: &str) -> bool {
    rec.groups.iter().any(|g| g == group)
}
