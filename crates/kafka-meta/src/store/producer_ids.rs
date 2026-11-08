//! Producer ids: unique for the life of the data directory.

use redb::ReadableTable;

use super::{COUNTERS, MetaStore, lock};
use crate::MetaError;

/// How many ids one flush reserves.
const BLOCK: i64 = 1000;
const COUNTER: &str = "producer_ids";

/// The ids reserved and not yet handed out.
#[derive(Debug, Default)]
pub(super) struct Block {
    next: i64,
    end: i64,
}

impl MetaStore {
    /// The next producer id. A block of ids is reserved durably at a time, so a flush is paid once for a
    /// thousand producers; the ids left in a block when the process stops are never used, which costs nothing
    /// (an id only has to be unique).
    ///
    /// # Errors
    /// A failed write.
    pub fn next_producer_id(&self) -> Result<i64, MetaError> {
        let mut block = lock(&self.ids);
        if block.next == block.end {
            let first = self.reserve(BLOCK)?;
            *block = Block {
                next: first,
                end: first + BLOCK,
            };
        }
        let id = block.next;
        block.next += 1;
        Ok(id)
    }

    /// Reserves `n` ids and returns the first.
    fn reserve(&self, n: i64) -> Result<i64, MetaError> {
        let txn = self.begin()?;
        let first = {
            let mut counters = txn.open_table(COUNTERS)?;
            let first = counters
                .get(COUNTER)?
                .map_or(0, |v| i64::try_from(v.value()).unwrap_or(i64::MAX));
            let after = u64::try_from(first.saturating_add(n)).unwrap_or(u64::MAX);
            counters.insert(COUNTER, after)?;
            first
        };
        txn.commit()?;
        Ok(first)
    }
}
