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

/// Each producer's window of recent batches (reads the log's history the first time, like [`view`]).
pub(super) fn windows<F: Fs>(
    log: &PartitionLog<F>,
    memory: &mut Memory,
) -> Result<Vec<crate::producers::Window>, StorageError> {
    if !memory.loaded {
        relearn(log, memory)?;
    }
    Ok(memory.producers.windows())
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kafka_storage::sim::SimFs;
    use kafka_storage::{LogConfig, Trust};
    use kafka_wire::batch::build::BatchBuilder;
    use kafka_wire::batch::{Batch, Marker, TimestampType};
    use kafka_wire::ids::TimestampMs;

    use super::super::Trim;
    use super::*;
    use crate::producers::Producers;

    fn memory(index: TxnIndex) -> Memory {
        Memory {
            producers: Producers::default(),
            loaded: true,
            txns: shared(index),
            cleaned_to: 0,
        }
    }

    fn aborted_held(memory: &Memory) -> usize {
        let index = memory.txns.read().unwrap();
        index.aborted_between(0, i64::MAX).len()
    }

    #[test]
    fn moving_the_start_up_lets_go_of_the_aborted_transactions_that_ended_below_it() {
        let fs = SimFs::new();
        kafka_storage::fs::create_dirs_durably(&fs, std::path::Path::new("/data")).unwrap();
        let mut log = PartitionLog::open(
            fs,
            PathBuf::from("/data/t-0"),
            LogConfig::default(),
            Trust::Vouched,
        )
        .unwrap();
        for _ in 0..3 {
            let batch = Batch::parse(BatchBuilder::new().records(10).build()).unwrap();
            let now = TimestampMs::from_wire(1);
            log.append(&[batch], now, TimestampType::Create).unwrap();
        }
        // Two aborted transactions: one that ended at offset 7, one that began at 20 and ended at 28.
        let mut index = TxnIndex::default();
        for (producer, first, last) in [(1, 2, 7), (2, 20, 28)] {
            index.write(producer, 0, first);
            index.marker(producer, last, Marker::Abort);
        }
        let mut memory = memory(index);
        assert_eq!(aborted_held(&memory), 2);
        let to = Offset::new(15);
        assert_eq!(
            trim(&mut log, &mut memory, Trim::Before(to)).unwrap().get(),
            15
        );
        assert_eq!(
            aborted_held(&memory),
            1,
            "the one that ended at 7 is below 15"
        );
        let to = Offset::new(29);
        assert_eq!(
            trim(&mut log, &mut memory, Trim::Before(to)).unwrap().get(),
            29
        );
        assert_eq!(aborted_held(&memory), 0);
    }
}
