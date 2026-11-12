//! A keyed collection of records that lives in memory and in one redb table.
//!
//! The service changes records in memory (it knows nothing of the disk). [`Records::get_mut`], [`Records::insert`] and
//! [`Records::remove`] note the key; after the call, [`Records::stage`] encodes the noted records, compares each with the bytes
//! last written, and returns only the rows that differ. A change cannot be made and forgotten: the only way to a `&mut T` is
//! the one that notes the key.

use std::collections::{BTreeMap, BTreeSet};

use redb::{ReadTransaction, ReadableTable, TableDefinition, WriteTransaction};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{StoreError, record};

/// A record and the bytes last written for it.
#[derive(Debug)]
struct Row<T> {
    value: T,
    written: Vec<u8>,
}

/// The rows one call changed, ready for the writer.
#[derive(Debug, Default)]
pub struct Changes {
    table: &'static str,
    puts: Vec<(String, Vec<u8>)>,
    deletes: Vec<String>,
}

impl Changes {
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
                    written: bytes.value().to_vec(),
                },
            );
        }
        Ok(records)
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
        let written = self
            .items
            .remove(&key)
            .map(|row| row.written)
            .unwrap_or_default();
        self.items.insert(key, Row { value, written });
    }

    /// Removes and returns the record under `key`.
    pub fn remove(&mut self, key: &str) -> Option<T> {
        let row = self.items.remove(key)?;
        self.touched.remove(key);
        if !row.written.is_empty() {
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
            if bytes != row.written {
                row.written.clone_from(&bytes);
                changes.puts.push((key, bytes));
            }
        }
        Ok(changes)
    }
}
