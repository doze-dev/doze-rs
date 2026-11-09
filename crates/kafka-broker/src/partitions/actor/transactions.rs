//! The writer's view of the partition's transactions: told of each transactional batch and marker as it is written,
//! and read from the log the first time it is needed.

use kafka_storage::fs::Fs;
use kafka_storage::{Appended, PartitionLog, StorageError};
use kafka_wire::batch::{Batch, Marker};
use kafka_wire::ids::Offset;

use super::{Append, Memory, TxnView, relearn};
use crate::partitions::gone;
use crate::txn_index::TxnIndex;

/// Appends the batch; a transactional batch or a marker is told to the transaction index too.
pub(super) fn write<F: Fs>(
    log: &mut PartitionLog<F>,
    memory: &mut Memory,
    a: &Append,
) -> Result<Appended, StorageError> {
    let transactional =
        a.batch.attributes().is_transactional() || a.batch.attributes().is_control();
    if transactional {
        transactions(log, memory)?;
    }
    let appended = log.append(std::slice::from_ref(&a.batch), a.now, a.clock)?;
    if transactional {
        note(memory, &a.batch, appended.first_offset.get());
    }
    Ok(appended)
}

/// The transactions in the log, read once from its batches.
fn transactions<F: Fs>(log: &PartitionLog<F>, memory: &mut Memory) -> Result<(), StorageError> {
    if memory.txns.is_some() {
        return Ok(());
    }
    let mut index = TxnIndex::default();
    log.reader()
        .visit_batches(log.reader().start_offset(), |peek, bytes| {
            let Some(p) = peek.producer else { return };
            if peek.attributes.is_control() {
                if let Some(kind) = Marker::read(bytes) {
                    index.marker(p.id.get(), peek.base_offset, kind);
                }
            } else if peek.attributes.is_transactional() {
                index.write(p.id.get(), p.epoch.get(), peek.base_offset);
            }
        })?;
    memory.txns = Some(index);
    Ok(())
}

/// Tells the transaction index of a batch just written at `offset`.
fn note(memory: &mut Memory, batch: &Batch, offset: i64) {
    let (Some(index), Some(p)) = (memory.txns.as_mut(), batch.producer()) else {
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

pub(super) fn view<F: Fs>(
    log: &PartitionLog<F>,
    memory: &mut Memory,
) -> Result<TxnView, StorageError> {
    transactions(log, memory)?;
    if !memory.loaded {
        relearn(log, memory)?;
    }
    let reader = log.reader();
    let end = reader.end_offset().get();
    let Some(index) = memory.txns.as_mut() else {
        return Err(gone());
    };
    index.forget_before(reader.start_offset().get());
    Ok(TxnView {
        last_stable: Offset::new(index.last_stable(end)).unwrap_or(Offset::ZERO),
        aborted: index.aborted_between(reader.start_offset().get(), end),
        open: index.open(),
        producers: memory.producers.snapshot(),
    })
}
