//! The metadata database: one redb file, opened over the file seam.

mod broker_configs;
mod lives;
mod offsets;
mod producer_ids;
mod registry;
mod topics;
mod txns;

pub use topics::random_topic_id;

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use doze_store::Store;
use kafka_storage::fs::Fs;
use redb::{TableDefinition, WriteTransaction};

use crate::MetaError;
use registry::Shared;

/// Topic name to its record.
pub(super) const TOPICS: TableDefinition<'static, &str, &[u8]> = TableDefinition::new("topics");
/// A group's committed offsets: see `offsets.rs` for the key.
pub(super) const OFFSETS: TableDefinition<'static, &[u8], &[u8]> = TableDefinition::new("offsets");
/// What is known of a group's members, for the offsets retention.
pub(super) const GROUP_LIVES: TableDefinition<'static, &str, &[u8]> =
    TableDefinition::new("group_lives");
/// Named counters.
pub(super) const TXNS: TableDefinition<'static, &str, &[u8]> = TableDefinition::new("txns");
/// The broker's dynamic configs: `scope/name` to the value as text.
pub(super) const BROKER_CONFIGS: TableDefinition<'static, &str, &str> =
    TableDefinition::new("broker_configs");

pub(super) const COUNTERS: TableDefinition<'static, &str, u64> = TableDefinition::new("counters");

const FILE: &str = "meta.redb";
/// redb's page cache. Everything the broker asks often is held in memory by the registry, so this only has to
/// carry a write's own pages.
const CACHE_BYTES: usize = 1 << 20;

/// The broker's metadata. Reads are served from memory; a write is durable (flushed) before it returns, and
/// a crash at any point leaves it done or not done, never half. Writes are one at a time.
#[derive(Debug)]
pub struct MetaStore {
    db: Store,
    /// What the topics are, in memory, changed only by a write that has committed.
    shared: Shared,
    /// Held for the whole of a topic change, from the checks to the registry update, so that two changes
    /// cannot both pass their checks.
    write: Mutex<()>,
    ids: Mutex<producer_ids::Block>,
}

impl MetaStore {
    /// Opens the database in `dir`, making it if the directory has none.
    ///
    /// The database is made under another name and moved into place once it is whole and flushed, so a
    /// crash while it is being made leaves no file that redb would have to make sense of.
    ///
    /// # Errors
    /// A file system or database failure, or a topic record this version cannot read.
    pub fn open<F: Fs>(fs: &F, dir: &Path) -> Result<Self, MetaError> {
        let db = Store::open(fs, dir, FILE, CACHE_BYTES, |txn| {
            txn.open_table(TOPICS)?;
            txn.open_table(COUNTERS)?;
            txn.open_table(OFFSETS)?;
            txn.open_table(GROUP_LIVES)?;
            txn.open_table(TXNS)?;
            txn.open_table(BROKER_CONFIGS)?;
            Ok(())
        })?;
        let shared = Shared::load(&db)?;
        Ok(Self {
            db,
            shared,
            write: Mutex::new(()),
            ids: Mutex::new(producer_ids::Block::default()),
        })
    }

    /// Closes the database, flushing it once more. (Dropping the store closes it too, but cannot say if that
    /// failed; the commits themselves were flushed when they were made, so a failure here costs only a
    /// longer start next time.)
    ///
    /// # Errors
    /// A failed flush.
    pub fn close(self) -> Result<(), MetaError> {
        Ok(self.db.close()?)
    }

    pub(super) fn begin(&self) -> Result<WriteTransaction, MetaError> {
        // Flushed at commit, whatever the library's default says.
        Ok(self.db.begin()?)
    }
}

/// A lock held for a short step; one that a panicking thread poisoned is still taken (the data under it is
/// only ever changed after the step that could panic).
pub(super) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
