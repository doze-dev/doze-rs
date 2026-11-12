//! The file: one redb database per region with four tables (the table records, the items, the index entries, the stream records),
//! written through the group-commit writer. Items are read from the file when a request needs them; nothing but the table
//! records is kept in memory.

use std::path::Path;

use doze_fs::fs::{Fs, RealFs};
use doze_store::records::Changes;
use doze_store::{Ack, GroupCommit, Op, Store, StoreError};
use redb::{ReadableDatabase, TableDefinition};

use crate::keys::Space;

/// redb's page cache: items are read when needed, and the idle heap is measured.
const CACHE_BYTES: usize = 1 << 20;
const FILE: &str = "dynamodb.redb";
pub(crate) const TABLES: &str = "tables";

type Def = TableDefinition<'static, &'static [u8], &'static [u8]>;

fn def(space: Space) -> Def {
    TableDefinition::new(space.table_name())
}

/// The file and its writer.
#[derive(Debug)]
pub(crate) struct Disk {
    store: Store,
    writer: GroupCommit,
}

/// Everything one call changes, applied in one transaction.
#[derive(Debug, Default)]
pub(crate) struct Writes {
    pub(crate) meta: Vec<Changes>,
    pub(crate) puts: Vec<(Space, Vec<u8>, Vec<u8>)>,
    pub(crate) dels: Vec<(Space, Vec<u8>)>,
    /// Every entry from the first key up to (not including) the second.
    pub(crate) ranges: Vec<(Space, Vec<u8>, Vec<u8>)>,
}

impl Writes {
    pub(crate) fn is_empty(&self) -> bool {
        self.meta.iter().all(Changes::is_empty)
            && self.puts.is_empty()
            && self.dels.is_empty()
            && self.ranges.is_empty()
    }

    pub(crate) fn put(&mut self, space: Space, key: Vec<u8>, value: Vec<u8>) {
        self.puts.push((space, key, value));
    }

    pub(crate) fn del(&mut self, space: Space, key: Vec<u8>) {
        self.dels.push((space, key));
    }
}

/// What to do with the next entry of a read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flow {
    Continue,
    Stop,
}

impl Disk {
    pub(crate) fn open(dir: &Path) -> Result<Self, StoreError> {
        let fs = RealFs;
        fs.create_dir_all(dir)?;
        let store = Store::open(&fs, dir, FILE, CACHE_BYTES, |txn| {
            txn.open_table(TableDefinition::<&str, &[u8]>::new(TABLES))?;
            for space in [Space::Items, Space::Index, Space::Stream] {
                txn.open_table(def(space))?;
            }
            Ok(())
        })?;
        let writer = GroupCommit::start(store.clone())?;
        Ok(Self { store, writer })
    }

    /// The table records, as the rows of the `tables` table.
    pub(crate) fn read(&self) -> Result<redb::ReadTransaction, StoreError> {
        Ok(self.store.begin_read()?)
    }

    /// The value filed under `key`.
    pub(crate) fn get(&self, space: Space, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let txn = self.store.begin_read()?;
        let table = txn.open_table(def(space))?;
        Ok(table.get(key)?.map(|v| v.value().to_vec()))
    }

    /// Calls `each` with the entries from `lo` up to (not including) `hi`, in order or in reverse, until it says stop.
    pub(crate) fn scan(
        &self,
        space: Space,
        lo: &[u8],
        hi: &[u8],
        reverse: bool,
        mut each: impl FnMut(&[u8], &[u8]) -> Flow,
    ) -> Result<(), StoreError> {
        if lo >= hi {
            return Ok(());
        }
        let txn = self.store.begin_read()?;
        let table = txn.open_table(def(space))?;
        let range = table.range::<&[u8]>(lo..hi)?;
        if reverse {
            for row in range.rev() {
                let (k, v) = row?;
                if each(k.value(), v.value()) == Flow::Stop {
                    break;
                }
            }
        } else {
            for row in range {
                let (k, v) = row?;
                if each(k.value(), v.value()) == Flow::Stop {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Queues the writes. The [`Ack`] resolves when they are on disk; `None` when there is nothing to write.
    pub(crate) fn submit(&self, writes: Writes) -> Option<Ack> {
        if writes.is_empty() {
            return None;
        }
        let op: Op = Box::new(move |txn| {
            for c in &writes.meta {
                c.apply(txn)?;
            }
            for (space, lo, hi) in &writes.ranges {
                let mut table = txn.open_table(def(*space))?;
                table.retain_in::<&[u8], _>(lo.as_slice()..hi.as_slice(), |_, _| false)?;
            }
            for (space, key) in &writes.dels {
                txn.open_table(def(*space))?.remove(key.as_slice())?;
            }
            for (space, key, value) in &writes.puts {
                txn.open_table(def(*space))?
                    .insert(key.as_slice(), value.as_slice())?;
            }
            Ok(())
        });
        Some(self.writer.submit(op))
    }

    pub(crate) fn close(self) {
        self.writer.shutdown();
        // WHY: every commit was flushed when it was made, so a failure to close costs only a longer start next time.
        drop(self.store.close());
    }
}
