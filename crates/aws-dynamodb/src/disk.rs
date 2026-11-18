//! The file: one redb database per region with eight tables (the table records, the Contributor Insights settings, the other
//! records of `extras.rs`, the items, the index entries, the stream records, the items of backups, the point-in-time recovery log),
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
pub(crate) const FILE: &str = "dynamodb.redb";
pub(crate) const TABLES: &str = "tables";
/// The Contributor Insights settings, by table (and `table/index`).
pub(crate) const INSIGHTS: &str = "insights";
/// Backups, exports, imports, resource policies and Kinesis streaming destinations (`extras.rs`).
pub(crate) const EXTRAS: &str = "extras";

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
            for meta in [TABLES, INSIGHTS, EXTRAS] {
                txn.open_table(TableDefinition::<&str, &[u8]>::new(meta))?;
            }
            for space in Space::ALL {
                txn.open_table(def(space))?;
            }
            Ok(())
        })?;
        // A file made before Contributor Insights, backups and the recovery log were kept has no tables for them.
        let read = store.begin_read()?;
        let missing_meta = [INSIGHTS, EXTRAS]
            .into_iter()
            .any(|m| read.open_table(TableDefinition::<&str, &[u8]>::new(m)).is_err());
        let missing_space = Space::ALL
            .into_iter()
            .any(|s| read.open_table(def(s)).is_err());
        drop(read);
        if missing_meta || missing_space {
            let txn = store.begin()?;
            for meta in [INSIGHTS, EXTRAS] {
                txn.open_table(TableDefinition::<&str, &[u8]>::new(meta))?;
            }
            for space in Space::ALL {
                txn.open_table(def(space))?;
            }
            txn.commit()?;
        }
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

    /// The entries from `lo` up to (not including) `hi`, and the bytes their keys and values hold.
    pub(crate) fn measure(
        &self,
        space: Space,
        lo: &[u8],
        hi: &[u8],
    ) -> Result<(u64, u64), StoreError> {
        let (mut rows, mut bytes) = (0, 0);
        self.scan(space, lo, hi, false, |k, v| {
            rows += 1;
            bytes += (k.len() + v.len()) as u64;
            Flow::Continue
        })?;
        Ok((rows, bytes))
    }

    /// How much of the file holds data.
    pub(crate) fn space(&self) -> Result<doze_store::Space, StoreError> {
        self.store.space()
    }

    /// Gives the space deletes freed back to the disk, after the writer has finished what it was given.
    pub(crate) fn compact(&mut self) -> Result<doze_store::Compacted, StoreError> {
        self.writer.paused(&mut self.store, Store::compact)?
    }

    /// Stops the writer after it has finished what it was given, gives back the space deletes left when that is worth it, and
    /// closes the database.
    pub(crate) fn close(mut self) {
        self.writer.shutdown();
        // WHY: one that cannot run now runs at a later close; the data is whole either way.
        drop(self.store.compact_if_worth());
        // WHY: every commit was flushed when it was made, so a failure to close costs only a longer start next time.
        drop(self.store.close());
    }
}
