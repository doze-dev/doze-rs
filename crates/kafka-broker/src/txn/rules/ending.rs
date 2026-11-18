//! How a transaction is ended at either transaction version: what EndTxn v5 is answered (KIP-890's second part), and what
//! each decision, the broker's own abort and the completion write to the record.

use kafka_meta::{TxnRecord, TxnState, TxnVersion};
use kafka_wire::ResponseError as E;

use super::End;

/// What an EndTxn of KIP-890's second part (version 5) is to do, owner checks included: Kafka 4.4.0's
/// `TransactionCoordinator.endTransaction` for `TV_2`, held to its answers in the oracle. Every end raises the epoch, so the
/// epoch a request carries says which end it is: the current one asks for the next end, the one before (or the last of
/// a producer id that was handed over, at 0 of the new one) is a retry of the end that raised it.
///
/// | state | current, the same | current, the other | retry, the same | retry, the other |
/// |---|---|---|---|---|
/// | Ongoing | decided | decided | PRODUCER_FENCED | PRODUCER_FENCED |
/// | Empty | abort decided, commit INVALID_TXN_STATE | | PRODUCER_FENCED | PRODUCER_FENCED |
/// | CompleteCommit | commit INVALID_TXN_STATE, abort decided | | commit success | abort INVALID_TXN_STATE |
/// | CompleteAbort | abort decided, commit INVALID_TXN_STATE | | abort success | commit INVALID_TXN_STATE |
/// | PrepareCommit / PrepareAbort | PRODUCER_FENCED | PRODUCER_FENCED | carried out, then as Complete | |
///
/// An abort decided where nothing was begun (Empty, Complete) raises the epoch and writes no marker. Another producer id is
/// INVALID_PRODUCER_ID_MAPPING; any other epoch PRODUCER_FENCED. A success that decides nothing is answered with the
/// record's producer id and epoch.
#[must_use]
pub(crate) fn end_bumping(rec: &TxnRecord, producer: i64, epoch: i16, commit: bool) -> End {
    let (asked, current) = (i32::from(epoch), i32::from(rec.epoch));
    let retry_on_overflow = rec.prev_producer_id >= 0
        && producer == rec.prev_producer_id
        && epoch == i16::MAX - 1
        && rec.epoch == 0;
    let retry_on_bump = current == asked + 1;
    if rec.producer_id < 0 || (producer != rec.producer_id && !retry_on_overflow) {
        return End::Answer(Some(E::InvalidProducerIdMapping));
    }
    let valid = match rec.state {
        TxnState::Ongoing | TxnState::Empty => asked == current,
        TxnState::PrepareCommit | TxnState::PrepareAbort => retry_on_bump,
        TxnState::CompleteCommit | TxnState::CompleteAbort => {
            retry_on_bump || retry_on_overflow || asked == current
        }
    };
    if !valid {
        return End::Answer(Some(E::ProducerFenced));
    }
    let retry = retry_on_bump || retry_on_overflow;
    match (rec.state, commit) {
        (TxnState::Ongoing, _) | (TxnState::Empty, false) => End::Decide { commit },
        (TxnState::CompleteCommit, true) | (TxnState::CompleteAbort, false) if retry => {
            End::Answer(None)
        }
        (TxnState::CompleteCommit | TxnState::CompleteAbort, false) if !retry => {
            End::Decide { commit: false }
        }
        (TxnState::PrepareCommit, asked) => End::Finish {
            decided: true,
            asked,
        },
        (TxnState::PrepareAbort, asked) => End::Finish {
            decided: false,
            asked,
        },
        _ => End::Answer(Some(E::InvalidTxnState)),
    }
}

/// The record of a decision that raises the epoch (EndTxn v5, and the broker's own abort at `transaction.version` 2):
/// the markers carry the raised epoch, the producer is told it, the epoch it had is kept as the last (a producer asking
/// again with it is answered, KIP-360), and when it was the last of the producer id, the id the transaction hands over
/// to at its end is `next`. An abort of nothing (from Empty or a Complete state) begins now. Kafka's
/// `TransactionMetadata.prepareAbortOrCommit` for `TV_2`; an epoch that is already `i16::MAX` (only a classic fence can
/// leave one) is not raised past it.
pub(crate) fn decide_bumping(rec: &mut TxnRecord, commit: bool, next: Option<i64>, now_ms: i64) {
    let had = rec.epoch;
    rec.epoch = had.saturating_add(1);
    rec.last_epoch = had;
    rec.next_producer_id = next.unwrap_or(-1);
    rec.client_version = TxnVersion::EpochBump;
    if rec.state != TxnState::Ongoing {
        rec.started_ms = now_ms;
    }
    rec.state = if commit {
        TxnState::PrepareCommit
    } else {
        TxnState::PrepareAbort
    };
}

/// The record of a classic decision (EndTxn v0 to v4): the epoch stays.
pub(crate) fn decide_classic(rec: &mut TxnRecord, commit: bool) {
    rec.client_version = TxnVersion::Classic;
    rec.next_producer_id = -1;
    rec.state = if commit {
        TxnState::PrepareCommit
    } else {
        TxnState::PrepareAbort
    };
}

/// The record of the broker's own abort of an open transaction (a timeout, a new owner), at the `transaction.version` it
/// finalizes. The epoch is raised first, so the markers and whatever the producer tries next carry the new one. At the
/// classic level the epoch before is forgotten (Kafka's `endTransactionWithTV1` sets it to -1 for a fence): the producer
/// it fenced cannot take the id back by saying the epoch it had. At level 2 the abort is EndTxn v5's
/// ([`decide_bumping`]): the epoch it had is kept, and that producer may ask for the id again with it (KIP-360), as Kafka
/// 4.4.0 answered in the oracle.
pub(crate) fn fence(rec: &mut TxnRecord, level: TxnVersion, next: Option<i64>, now_ms: i64) {
    match level {
        TxnVersion::EpochBump => decide_bumping(rec, false, next, now_ms),
        TxnVersion::Classic => {
            rec.epoch = rec.epoch.saturating_add(1);
            rec.last_epoch = -1;
            decide_classic(rec, false);
        }
    }
}

/// The record of a carried-out decision: the transaction is complete, and if its epochs ran out the producer id it was to
/// hand over to takes over at epoch 0 (the one before is kept, with its last epoch, for a retry; Kafka's
/// `TransactionMetadata.prepareComplete`).
pub(crate) fn complete(rec: &mut TxnRecord, commit: bool) {
    rec.state = if commit {
        TxnState::CompleteCommit
    } else {
        TxnState::CompleteAbort
    };
    if rec.next_producer_id >= 0 {
        rec.prev_producer_id = rec.producer_id;
        rec.producer_id = rec.next_producer_id;
        rec.epoch = 0;
        rec.next_producer_id = -1;
    }
    rec.partitions.clear();
    rec.groups.clear();
    rec.offsets.clear();
}

#[cfg(test)]
mod tests;
