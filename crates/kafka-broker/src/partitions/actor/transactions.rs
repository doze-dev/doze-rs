//! The writer's part in the partition's transactions: it tells the shared index of each transactional batch and marker as
//! it is written, and notes on disk that the log holds transactions before the first is.
//!
//! The index is shared with the readers (it is behind a lock held only for the instants of reading or changing it), so a
//! `read_committed` fetch does not queue behind appends. The lock is taken before a transactional batch is appended and
//! let go after the index has been told of it: a reader that sees the batch in the log therefore finds it in the index.

use std::sync::{Arc, PoisonError, RwLock};

use kafka_storage::fs::Fs;
use kafka_storage::{Appended, LogReader, PartitionLog, StorageError};
use kafka_wire::batch::{Batch, Marker};
use kafka_wire::ids::Offset;

use super::{Append, Memory, TxnView, relearn};
use crate::txn_index::TxnIndex;

/// Appends the batch; a transactional batch or a marker is told to the transaction index too.
pub(super) fn write<F: Fs>(
    log: &mut PartitionLog<F>,
    memory: &mut Memory,
    a: &Append,
) -> Result<Appended, StorageError> {
    let transactional =
        a.batch.attributes().is_transactional() || a.batch.attributes().is_control();
    if !transactional {
        return log.append(std::slice::from_ref(&a.batch), a.now, a.clock);
    }
    // The note is durable before the batch is written: a log without it is taken to hold no transactions.
    log.note_transactions()?;
    let mut index = memory.txns.write().unwrap_or_else(PoisonError::into_inner);
    let appended = log.append(std::slice::from_ref(&a.batch), a.now, a.clock)?;
    note(&mut index, &a.batch, appended.first_offset.get());
    index.forget_before(log.start_offset().get());
    Ok(appended)
}

/// The transactions in a log, read from its batches: for a log that holds transactions, when it is opened.
///
/// # Errors
/// A failed read of the log.
pub(crate) fn read_index<F: Fs>(reader: &LogReader<F>) -> Result<TxnIndex, StorageError> {
    let mut index = TxnIndex::default();
    reader.visit_batches(reader.start_offset(), |peek, bytes| {
        let Some(p) = peek.producer else { return };
        if peek.attributes.is_control() {
            if let Some(kind) = Marker::read(bytes) {
                index.marker(p.id.get(), peek.base_offset, kind);
            }
        } else if peek.attributes.is_transactional() {
            index.write(p.id.get(), p.epoch.get(), peek.base_offset);
        }
    })?;
    Ok(index)
}

/// Tells the transaction index of a batch just written at `offset`.
fn note(index: &mut TxnIndex, batch: &Batch, offset: i64) {
    let Some(p) = batch.producer() else {
        return;
    };
    if batch.attributes().is_control() {
        if let Some(kind) = Marker::read(batch.as_bytes()) {
            index.marker(p.id.get(), offset, kind);
        }
    } else if batch.attributes().is_transactional() {
        index.write(p.id.get(), p.epoch.get(), offset);
    }
}

/// The open transactions and the producers the partition remembers.
pub(super) fn view<F: Fs>(
    log: &PartitionLog<F>,
    memory: &mut Memory,
) -> Result<TxnView, StorageError> {
    if !memory.loaded {
        relearn(log, memory)?;
    }
    let open = memory
        .txns
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .open();
    Ok(TxnView {
        open,
        producers: memory.producers.snapshot(),
    })
}

/// A new, shared index.
pub(crate) fn shared(index: TxnIndex) -> Arc<RwLock<TxnIndex>> {
    Arc::new(RwLock::new(index))
}

/// Moves the start of the log up, and lets go of what the transaction index and the producers remembered below it.
pub(super) fn trim<F: Fs>(
    log: &mut PartitionLog<F>,
    memory: &mut Memory,
    how: super::Trim,
) -> Result<Offset, StorageError> {
    let start = match how {
        super::Trim::Retention(policy, now) => {
            log.enforce_retention(policy, now)?;
            log.start_offset()
        }
        super::Trim::Before(to) => log.delete_before(to)?,
    };
    memory
        .txns
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .forget_before(start.get());
    memory.producers.forget_below(start.get());
    Ok(start)
}
