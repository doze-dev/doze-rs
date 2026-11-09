//! What a transactional id's record allows, decided from the record alone.

use kafka_meta::{TxnRecord, TxnState};
use kafka_wire::ResponseError as E;

/// The longest `transaction.timeout.ms` a producer may ask for (Kafka's `transaction.max.timeout.ms`).
pub(crate) const MOST_TIMEOUT_MS: i32 = 15 * 60 * 1000;

/// Whether a request's producer id and epoch are the ones that own the transactional id. A lower epoch has been
/// fenced by a newer producer.
pub(crate) fn owns(rec: &TxnRecord, producer: i64, epoch: i16) -> Option<E> {
    if rec.producer_id < 0 || producer != rec.producer_id {
        Some(E::InvalidProducerIdMapping)
    } else if epoch < rec.epoch {
        Some(E::ProducerFenced)
    } else if epoch > rec.epoch {
        Some(E::InvalidProducerEpoch)
    } else {
        None
    }
}

/// Partitions or offsets may be added unless the transaction is being ended.
pub(crate) fn can_add(rec: &TxnRecord) -> Option<E> {
    matches!(rec.state, TxnState::PrepareCommit | TxnState::PrepareAbort)
        .then_some(E::ConcurrentTransactions)
}

/// A settled record becomes an open transaction, starting from nothing.
pub(crate) fn begin(rec: &mut TxnRecord, now_ms: i64) {
    if rec.state != TxnState::Ongoing {
        rec.state = TxnState::Ongoing;
        rec.started_ms = now_ms;
        rec.partitions.clear();
        rec.groups.clear();
        rec.offsets.clear();
    }
}

/// What an EndTxn is to do, from the record's state and the decision asked for. What Apache Kafka 4.3.1 answers
/// (recorded: `endtxn-decisions`, `endtxn-while-preparing`):
///
/// | state | the same decision | the other |
/// |---|---|---|
/// | Ongoing | carried out | carried out |
/// | PrepareCommit / PrepareAbort | CONCURRENT_TRANSACTIONS until carried out | likewise, then INVALID_TXN_STATE |
/// | CompleteCommit / CompleteAbort | success | INVALID_TXN_STATE |
/// | Empty | INVALID_TXN_STATE | INVALID_TXN_STATE |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum End {
    /// Store the decision, then carry it out.
    Decide { commit: bool },
    /// A decision is stored and not yet carried out: carry it out; then the answer is success if `asked` is what was
    /// decided, INVALID_TXN_STATE if not.
    Finish { decided: bool, asked: bool },
    /// Answer at once.
    Answer(Option<E>),
}

#[must_use]
pub(crate) fn end(rec: &TxnRecord, commit: bool) -> End {
    match rec.state {
        TxnState::Ongoing => End::Decide { commit },
        TxnState::PrepareCommit => End::Finish {
            decided: true,
            asked: commit,
        },
        TxnState::PrepareAbort => End::Finish {
            decided: false,
            asked: commit,
        },
        TxnState::CompleteCommit => End::Answer((!commit).then_some(E::InvalidTxnState)),
        TxnState::CompleteAbort => End::Answer(commit.then_some(E::InvalidTxnState)),
        TxnState::Empty => End::Answer(Some(E::InvalidTxnState)),
    }
}

/// Whether the open transaction has outlived its timeout, counted from when the record last changed.
pub(crate) fn expired(rec: &TxnRecord, now_ms: i64) -> bool {
    rec.state == TxnState::Ongoing
        && now_ms.saturating_sub(rec.updated_ms) > i64::from(rec.timeout_ms)
}

/// Whether the id has nothing open and has not changed for `limit_ms`: it can be forgotten.
pub(crate) fn idle(rec: &TxnRecord, now_ms: i64, limit_ms: i64) -> bool {
    matches!(
        rec.state,
        TxnState::Empty | TxnState::CompleteCommit | TxnState::CompleteAbort
    ) && now_ms.saturating_sub(rec.updated_ms) > limit_ms
}

/// Whether a batch from `producer` at `epoch` to this partition is part of a transaction that may take it: the producer
/// owns the id, a transaction is open and the partition was added to it. Otherwise the batch would sit in the log with
/// no marker ever coming for it, holding the last stable offset still for ever.
pub(crate) fn may_write(
    rec: &TxnRecord,
    producer: i64,
    epoch: i16,
    topic: &str,
    partition: i32,
) -> Result<(), E> {
    match owns(rec, producer, epoch) {
        // What a partition reports; the client treats both as fenced.
        Some(E::ProducerFenced) => return Err(E::InvalidProducerEpoch),
        Some(e) => return Err(e),
        None => {}
    }
    if rec.state == TxnState::Ongoing
        && rec
            .partitions
            .iter()
            .any(|(t, p)| t == topic && *p == partition)
    {
        Ok(())
    } else {
        Err(E::InvalidTxnState)
    }
}

#[cfg(test)]
mod tests;
