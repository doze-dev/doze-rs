//! A keyed collection of records that lives in memory and in one redb table.
//!
//! The service changes records in memory (it knows nothing of the disk). [`Records::get_mut`], [`Records::insert`] and
//! [`Records::remove`] note the key; after the call, [`Records::stage`] encodes the noted records, compares each with what was
//! last written, and returns only the rows that differ. A change cannot be made and forgotten: the only way to a `&mut T` is
//! the one that notes the key.
//!
//! What was last written is kept as its length and a 64-bit hash (`Seen`), not as the bytes: the bytes doubled every table's
//! memory (weight.md, "Workstream 7a").

use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hasher;
use std::num::NonZeroU32;

use redb::{ReadTransaction, ReadableTable, TableDefinition, WriteTransaction};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::pending::Change;
use crate::{StoreError, record};

/// A record, and what was last written for it (`None`: nothing yet, the record is not on disk).
#[derive(Debug)]
struct Row<T> {
    value: T,
    written: Option<Seen>,
}

/// The encoded bytes of a record as last written, as their length and a hash: 16 bytes in place of a copy of the record.
///
/// The hash is std's SipHash-1-3 (`DefaultHasher`, fixed keys; it is never stored, so a change of it between Rust versions
/// costs one rewrite of each record at most). Two different encodings of one length that hash alike would skip a write; by
/// chance that is one in 2^64 a change, against a disk that fails far more often. A keyed hash, so no record can be made to
/// collide on purpose without the keys, and no new crate. It runs at about 1 GB/s, under the postcard encode it follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Seen {
    /// An encoded record is never empty (it starts with its version byte); a length past `u32::MAX` is kept as that.
    len: NonZeroU32,
    hash: u64,
}

impl Seen {
    fn of(bytes: &[u8]) -> Option<Self> {
        let len = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
        let mut h = std::hash::DefaultHasher::new();
        h.write(bytes);
        Some(Self {
            len: NonZeroU32::new(len)?,
            hash: h.finish(),
        })
    }
}

/// The rows one call changed, ready for the writer.
#[derive(Debug, Default)]
pub struct Changes {
    table: &'static str,
    puts: Vec<(String, Vec<u8>)>,
    deletes: Vec<String>,
}

impl Changes {
    /// Rows written straight to `table`, for a service that keeps a table too large to hold in memory and reads it with
    /// [`Db::scan`](crate::Db::scan): the bytes of each row are the caller's (see [`record::encode`]).
    #[must_use]
    pub fn of_rows(
        table: &'static str,
        puts: Vec<(String, Vec<u8>)>,
        deletes: Vec<String>,
    ) -> Self {
        Self {
            table,
            puts,
            deletes,
        }
    }

    /// Whether there is nothing to write.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.puts.is_empty() && self.deletes.is_empty()
    }

    /// How many rows are written or deleted.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.puts.len() + self.deletes.len()
    }

    /// The changes to `table`, in the order [`Changes::apply`] makes them.
    pub fn seen(&self, table: &str, f: &mut dyn FnMut(Change<'_>)) {
        if self.table != table {
            return;
        }
        for key in &self.deletes {
            f(Change::Delete(key.as_bytes()));
        }
        for (key, bytes) in &self.puts {
            f(Change::Put(key.as_bytes(), bytes));
        }
    }

    /// Writes the rows, in the transaction of the batch they land in.
    ///
    /// # Errors
    /// A failed write.
    pub fn apply(&self, txn: &WriteTransaction) -> Result<(), StoreError> {
        let mut table = txn.open_table(TableDefinition::<&str, &[u8]>::new(self.table))?;
        for key in &self.deletes {
            table.remove(key.as_str())?;
        }
        for (key, bytes) in &self.puts {
            table.insert(key.as_str(), bytes.as_slice())?;
        }
        Ok(())
    }
}

/// The records of one table.
#[derive(Debug)]
pub struct Records<T> {
    table: &'static str,
    what: &'static str,
    version: u8,
    items: BTreeMap<String, Row<T>>,
    touched: BTreeSet<String>,
    gone: BTreeSet<String>,
}

impl<T: Serialize + DeserializeOwned> Records<T> {
    /// An empty collection for table `table`, whose records are called `what` in an error and carry `version`.
    #[must_use]
    pub fn new(table: &'static str, what: &'static str, version: u8) -> Self {
        Self {
            table,
            what,
            version,
            items: BTreeMap::new(),
            touched: BTreeSet::new(),
            gone: BTreeSet::new(),
        }
    }

    /// Reads every record of the table.
    ///
    /// # Errors
    /// A failed read, or a record this version cannot read.
    pub fn load(
        table: &'static str,
        what: &'static str,
        version: u8,
        read: &ReadTransaction,
    ) -> Result<Self, StoreError> {
        let mut records = Self::new(table, what, version);
        let rows = read.open_table(TableDefinition::<&str, &[u8]>::new(table))?;
        for row in rows.iter()? {
            let (key, bytes) = row?;
            let value = record::decode(what, version, bytes.value())?;
            records.items.insert(
                key.value().to_owned(),
                Row {
                    value,
                    written: Seen::of(bytes.value()),
                },
            );
        }
        Ok(records)
    }

    /// Reads every record of a table a later version added: a file written before the table existed has none, and the
    /// collection starts empty (the first write makes the table).
    ///
    /// # Errors
    /// A failed read, or a record this version cannot read.
    pub fn load_or_new(
        table: &'static str,
        what: &'static str,
        version: u8,
        read: &ReadTransaction,
    ) -> Result<Self, StoreError> {
        match read.open_table(TableDefinition::<&str, &[u8]>::new(table)) {
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(Self::new(table, what, version)),
            Err(e) => Err(e.into()),
            Ok(_) => Self::load(table, what, version, read),
        }
    }

    /// Reads the records of the table and keeps in memory those `keep` says to, handed each record and the length of its row. The
    /// others stay on disk only: a service that keeps a summary of them reads one whole from the database when it needs it
    /// ([`crate::Db::get`] and [`crate::record::decode`]).
    ///
    /// # Errors
    /// A failed read, or a record this version cannot read.
    pub fn load_where(
        table: &'static str,
        what: &'static str,
        version: u8,
        read: &ReadTransaction,
        mut keep: impl FnMut(&str, &T, usize) -> bool,
    ) -> Result<Self, StoreError> {
        let mut records = Self::new(table, what, version);
        let rows = read.open_table(TableDefinition::<&str, &[u8]>::new(table))?;
        for row in rows.iter()? {
            let (key, bytes) = row?;
            let value = record::decode(what, version, bytes.value())?;
            if keep(key.value(), &value, bytes.value().len()) {
                let written = Seen::of(bytes.value());
                records
                    .items
                    .insert(key.value().to_owned(), Row { value, written });
            }
        }
        Ok(records)
    }

    /// Lets go of the record under `key` in memory, as it was last staged: it stays on disk (or queued for it), and is not
    /// deleted. Answers it and the length of its row; `None` when it is not here or was never staged.
    pub fn evict(&mut self, key: &str) -> Option<(T, usize)> {
        let len = self.items.get(key)?.written?.len.get();
        if self.touched.contains(key) {
            // Changed since it was staged: it must be written first.
            return None;
        }
        let row = self.items.remove(key)?;
        Some((row.value, usize::try_from(len).unwrap_or(usize::MAX)))
    }

    /// Deletes the row under `key` from disk at the next [`Records::stage`], whether or not the record is held in memory (one
    /// [`Records::evict`] let go of).
    pub fn delete_stored(&mut self, key: &str) {
        self.items.remove(key);
        self.touched.remove(key);
        self.gone.insert(key.to_owned());
    }

    /// The record under `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&T> {
        self.items.get(key).map(|row| &row.value)
    }

    /// The record under `key`, to be changed: it is written after the call if it differs.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut T> {
        let row = self.items.get_mut(key)?;
        if !self.touched.contains(key) {
            self.touched.insert(key.to_owned());
        }
        Some(&mut row.value)
    }

    /// Puts a record, replacing any under `key`.
    pub fn insert(&mut self, key: String, value: T) {
        self.gone.remove(&key);
        self.touched.insert(key.clone());
        let written = self.items.remove(&key).and_then(|row| row.written);
        self.items.insert(key, Row { value, written });
    }

    /// Removes and returns the record under `key`.
    pub fn remove(&mut self, key: &str) -> Option<T> {
        let row = self.items.remove(key)?;
        self.touched.remove(key);
        if row.written.is_some() {
            self.gone.insert(key.to_owned());
        }
        Some(row.value)
    }

    /// Every record, in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
        self.items.iter().map(|(k, row)| (k.as_str(), &row.value))
    }

    /// The records whose key starts with `prefix`, in key order.
    pub fn with_prefix<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = (&'a str, &'a T)> {
        self.items
            .range::<str, _>((
                std::ops::Bound::Included(prefix),
                std::ops::Bound::Unbounded,
            ))
            .take_while(move |(k, _)| k.starts_with(prefix))
            .map(|(k, row)| (k.as_str(), &row.value))
    }

    /// How many records there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// What the call changed, as rows; the records are marked as written.
    ///
    /// # Errors
    /// A record that cannot be encoded.
    pub fn stage(&mut self) -> Result<Changes, StoreError> {
        let mut changes = Changes {
            table: self.table,
            puts: Vec::new(),
            deletes: std::mem::take(&mut self.gone).into_iter().collect(),
        };
        for key in std::mem::take(&mut self.touched) {
            let Some(row) = self.items.get_mut(&key) else {
                continue;
            };
            let bytes = record::encode(self.what, self.version, &row.value)?;
            let seen = Seen::of(&bytes);
            if seen != row.written {
                row.written = seen;
                changes.puts.push((key, bytes));
            }
        }
        Ok(changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staged(r: &mut Records<String>) -> (usize, usize) {
        let c = r.stage().unwrap();
        (c.puts.len(), c.deletes.len())
    }

    #[test]
    fn only_a_record_that_changed_is_written_again() {
        let mut r: Records<String> = Records::new("t", "thing", 1);
        r.insert("a".into(), "one".into());
        r.insert("b".into(), "two".into());
        assert_eq!(staged(&mut r), (2, 0));
        // Touched but the same: no write.
        r.get_mut("a");
        assert_eq!(staged(&mut r), (0, 0));
        // Touched and changed, even to a value of the same length.
        if let Some(v) = r.get_mut("a") {
            *v = "uno".into();
        }
        assert_eq!(staged(&mut r), (1, 0));
        // Put back as it was written: no write.
        r.insert("a".into(), "uno".into());
        assert_eq!(staged(&mut r), (0, 0));
        // Removed after it was written: a delete; removed before: nothing.
        r.remove("b");
        r.insert("c".into(), "new".into());
        r.remove("c");
        assert_eq!(staged(&mut r), (0, 1));
    }

    #[test]
    fn an_evicted_record_leaves_memory_not_the_disk_and_can_still_be_deleted() {
        let mut r: Records<String> = Records::new("t", "thing", 1);
        r.insert("a".into(), "one".into());
        assert_eq!(
            r.evict("a"),
            None,
            "not staged yet: it must be written first"
        );
        assert_eq!(staged(&mut r), (1, 0));
        assert_eq!(
            r.evict("a"),
            Some(("one".into(), 5)),
            "the version byte and four of postcard"
        );
        assert!(r.get("a").is_none());
        assert_eq!(staged(&mut r), (0, 0), "letting go writes nothing");
        r.delete_stored("a");
        assert_eq!(staged(&mut r), (0, 1));
    }

    #[test]
    fn a_row_keeps_sixteen_bytes_of_what_was_written_not_a_copy() {
        assert_eq!(std::mem::size_of::<Option<Seen>>(), 16);
        let big = vec![7u8; 100_000];
        let mut other = big.clone();
        other[50_000] = 8;
        assert_ne!(Seen::of(&big), Seen::of(&other));
        assert_eq!(Seen::of(&big), Seen::of(&big.clone()));
        assert_eq!(Seen::of(&[]), None);
    }
}
