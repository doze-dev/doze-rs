//! Rows too many to hold in memory: log events, stream records, metric samples.
//!
//! [`Records`](crate::records::Records) keeps every record in memory, which suits the few hundred a service has. A
//! [`RowTable`] is for the data that grows without bound: it is never loaded. A read goes to the database
//! ([`RowTable::scan`]); a write is noted and sent with the call's other rows ([`RowTable::stage`], then
//! [`Db::submit_with_rows`](crate::Db::submit_with_rows)), so the metadata and the rows of one call land in one transaction.
//!
//! Without a disk (the rules alone under test) the table is a `BTreeMap` and a write is visible at once.
//!
//! Keys are text that sorts as the rows should: callers write numbers as fixed-width hex.

use std::collections::BTreeMap;
use std::ops::Bound;

use redb::{TableDefinition, WriteTransaction};

use crate::{Db, StoreError};

/// One change to a row table.
#[derive(Debug)]
enum RowOp {
    Put(String, Vec<u8>),
    Delete(String),
    /// Every key in `from..to`.
    DeleteRange(String, String),
}

/// The changes one call made to a row table, ready for the writer.
#[derive(Debug, Default)]
pub struct Rows {
    table: &'static str,
    ops: Vec<RowOp>,
}

impl Rows {
    /// Whether there is nothing to write.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// How many changes there are (a range delete counts as one).
    #[must_use]
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Writes the changes, in the transaction of the batch they land in.
    ///
    /// # Errors
    /// A failed write.
    pub fn apply(&self, txn: &WriteTransaction) -> Result<(), StoreError> {
        let mut table = txn.open_table(TableDefinition::<&str, &[u8]>::new(self.table))?;
        for op in &self.ops {
            match op {
                RowOp::Put(key, bytes) => {
                    table.insert(key.as_str(), bytes.as_slice())?;
                }
                RowOp::Delete(key) => {
                    table.remove(key.as_str())?;
                }
                RowOp::DeleteRange(from, to) => {
                    table.retain_in::<&str, _>(from.as_str()..to.as_str(), |_, _| false)?;
                }
            }
        }
        Ok(())
    }
}

/// A table of rows read from the database and written in batches.
#[derive(Debug)]
pub struct RowTable {
    table: &'static str,
    /// The rows themselves when there is no disk.
    mem: Option<BTreeMap<String, Vec<u8>>>,
    ops: Vec<RowOp>,
}

impl RowTable {
    /// A table called `table`. `in_memory` keeps the rows in a map (no database behind it).
    #[must_use]
    pub fn new(table: &'static str, in_memory: bool) -> Self {
        Self {
            table,
            mem: in_memory.then(BTreeMap::new),
            ops: Vec::new(),
        }
    }

    /// Puts a row.
    pub fn put(&mut self, key: String, bytes: Vec<u8>) {
        match &mut self.mem {
            Some(mem) => {
                mem.insert(key, bytes);
            }
            None => self.ops.push(RowOp::Put(key, bytes)),
        }
    }

    /// Removes a row.
    pub fn delete(&mut self, key: &str) {
        match &mut self.mem {
            Some(mem) => {
                mem.remove(key);
            }
            None => self.ops.push(RowOp::Delete(key.to_owned())),
        }
    }

    /// Removes every row whose key is in `from..to`.
    pub fn delete_range(&mut self, from: &str, to: &str) {
        match &mut self.mem {
            Some(mem) => {
                let doomed: Vec<String> = mem
                    .range::<str, _>((Bound::Included(from), Bound::Excluded(to)))
                    .map(|(k, _)| k.clone())
                    .collect();
                for key in doomed {
                    mem.remove(&key);
                }
            }
            None => self
                .ops
                .push(RowOp::DeleteRange(from.to_owned(), to.to_owned())),
        }
    }

    /// The rows with keys in `from..to`, in key order (newest first when `reverse`), at most `limit` of them. `db` is the database
    /// to read when the table has one.
    ///
    /// # Errors
    /// A failed read.
    pub fn scan(
        &self,
        db: Option<&Db>,
        from: &str,
        to: &str,
        limit: usize,
        reverse: bool,
    ) -> Result<Vec<(String, Vec<u8>)>, StoreError> {
        if from >= to {
            return Ok(Vec::new());
        }
        if let Some(mem) = &self.mem {
            let rows = mem.range::<str, _>((Bound::Included(from), Bound::Excluded(to)));
            let own = |(k, v): (&String, &Vec<u8>)| (k.clone(), v.clone());
            return Ok(if reverse {
                rows.rev().take(limit).map(own).collect()
            } else {
                rows.take(limit).map(own).collect()
            });
        }
        let Some(db) = db else {
            return Ok(Vec::new());
        };
        let read = db.read()?;
        let table = read.open_table(TableDefinition::<&str, &[u8]>::new(self.table))?;
        let rows = table.range::<&str>(from..to)?;
        let mut out = Vec::new();
        let mut take = |row: Result<_, redb::StorageError>| -> Result<bool, StoreError> {
            let (k, v): (redb::AccessGuard<'_, &str>, redb::AccessGuard<'_, &[u8]>) = row?;
            out.push((k.value().to_owned(), v.value().to_vec()));
            Ok(out.len() < limit)
        };
        if reverse {
            for row in rows.rev() {
                if !take(row)? {
                    break;
                }
            }
        } else {
            for row in rows {
                if !take(row)? {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// The row under `key`.
    ///
    /// # Errors
    /// A failed read.
    pub fn get(&self, db: Option<&Db>, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        if let Some(mem) = &self.mem {
            return Ok(mem.get(key).cloned());
        }
        let Some(db) = db else {
            return Ok(None);
        };
        let read = db.read()?;
        let table = read.open_table(TableDefinition::<&str, &[u8]>::new(self.table))?;
        Ok(table.get(key)?.map(|v| v.value().to_vec()))
    }

    /// What the call changed, for the writer; the table forgets it.
    pub fn stage(&mut self) -> Rows {
        Rows {
            table: self.table,
            ops: std::mem::take(&mut self.ops),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("doze-store-rows-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        dir
    }

    fn fill(table: &mut RowTable) {
        for n in 0..10u8 {
            table.put(format!("k{n}"), vec![n]);
        }
    }

    #[test]
    fn a_map_table_scans_deletes_and_ranges() {
        let mut t = RowTable::new("t", true);
        fill(&mut t);
        let keys = |t: &RowTable, rev| -> Vec<String> {
            t.scan(None, "k0", "k9", 100, rev)
                .unwrap()
                .into_iter()
                .map(|(k, _)| k)
                .collect()
        };
        assert_eq!(keys(&t, false).len(), 9);
        assert_eq!(keys(&t, true).first().map(String::as_str), Some("k8"));
        t.delete_range("k2", "k5");
        t.delete("k7");
        assert_eq!(keys(&t, false), ["k0", "k1", "k5", "k6", "k8"]);
        assert_eq!(t.scan(None, "k0", "k9", 2, true).unwrap().len(), 2);
        assert_eq!(t.get(None, "k6").unwrap(), Some(vec![6]));
        assert!(t.stage().is_empty());
    }

    #[test]
    fn a_disk_table_is_written_with_the_batch_and_read_back() {
        let dir = temp("disk");
        let db = Db::open(&dir, "t.redb", &["rows"]).unwrap();
        let mut t = RowTable::new("rows", false);
        fill(&mut t);
        db.submit_with_rows(Vec::new(), vec![t.stage()])
            .unwrap()
            .wait()
            .unwrap();
        let all = t.scan(Some(&db), "k0", "k9", 100, false).unwrap();
        assert_eq!(all.len(), 9);
        let newest = t.scan(Some(&db), "k0", "k9", 3, true).unwrap();
        assert_eq!(newest.first().map(|(k, _)| k.as_str()), Some("k8"));
        t.delete_range("k0", "k5");
        t.delete("k9");
        db.submit_with_rows(Vec::new(), vec![t.stage()])
            .unwrap()
            .wait()
            .unwrap();
        let left: Vec<String> = t
            .scan(Some(&db), "k0", "kz", 100, false)
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(left, ["k5", "k6", "k7", "k8"]);
        assert_eq!(t.get(Some(&db), "k6").unwrap(), Some(vec![6]));
        assert_eq!(t.get(Some(&db), "k1").unwrap(), None);
        db.close();
        std::fs::remove_dir_all(&dir).ok();
    }
}
