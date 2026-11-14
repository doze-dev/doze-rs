//! The writer's part in cleaning: it knows what the log holds that the cleaner must respect (the first offset still open to a
//! transaction, the aborted transactions, what each producer last wrote), and it alone puts a round's results in place.

use std::collections::HashMap;
use std::sync::PoisonError;

use kafka_storage::compact::{AbortedTxn, Params, Planned};
use kafka_storage::fs::Fs;
use kafka_storage::{PartitionLog, StorageError};
use kafka_wire::ids::Offset;

use super::{Memory, relearn};

/// A round of cleaning for the log, if it is due one; if not, when it could next be.
pub(super) fn plan<F: Fs>(
    log: &PartitionLog<F>,
    memory: &mut Memory,
    params: &Params,
) -> Result<Planned<F>, StorageError> {
    if !memory.loaded {
        relearn(log, memory)?;
    }
    let index = memory.txns.read().unwrap_or_else(PoisonError::into_inner);
    let stable = index.last_stable(log.end_offset().get());
    let aborted = index
        .aborted_between(log.start_offset().get(), stable)
        .into_iter()
        .map(|a| AbortedTxn {
            producer: a.producer,
            first: a.first,
            last: a.last,
        })
        .collect();
    drop(index);
    let producers: HashMap<_, _> = memory.producers.lasts();
    log.plan_clean(
        params,
        Offset::new(stable).unwrap_or(Offset::ZERO),
        aborted,
        producers,
    )
}
