//! The schema registry's records (crate `kafka-registry`), kept beside the broker's metadata so the registry survives a
//! restart without a second store. They are opaque here: keys and values the registry encodes and reads back, written in
//! one transaction per registry operation.

use redb::{ReadableDatabase, ReadableTable, TableError};

/// Records as key and value.
pub type Records = Vec<(Vec<u8>, Vec<u8>)>;

use super::{MetaStore, SCHEMAS, lock};
use crate::MetaError;

impl MetaStore {
    /// Every schema-registry record, as key and value, in key order.
    ///
    /// # Errors
    /// The database cannot be read.
    pub fn registry_records(&self) -> Result<Records, MetaError> {
        let table = match self.db.begin_read()?.open_table(SCHEMAS) {
            Ok(table) => table,
            // A database made before the registry kept anything here.
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut all = Vec::new();
        for entry in table.iter()? {
            let (k, v) = entry?;
            all.push((k.value().to_vec(), v.value().to_vec()));
        }
        Ok(all)
    }

    /// Removes `deletes`, then writes `puts`, in one transaction that is durable when it returns.
    ///
    /// # Errors
    /// A failed write; nothing is then changed.
    pub fn registry_apply(
        &self,
        puts: &[(Vec<u8>, Vec<u8>)],
        deletes: &[Vec<u8>],
    ) -> Result<(), MetaError> {
        let _one_at_a_time = lock(&self.write);
        let txn = self.begin()?;
        {
            let mut table = txn.open_table(SCHEMAS)?;
            for key in deletes {
                table.remove(key.as_slice())?;
            }
            for (key, value) in puts {
                table.insert(key.as_slice(), value.as_slice())?;
            }
        }
        txn.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use kafka_storage::sim::SimFs;

    use crate::MetaStore;

    #[test]
    fn records_written_together_are_read_back_after_a_reopen_and_a_delete_goes() {
        let fs = SimFs::new();
        let dir = Path::new("/data");
        let meta = MetaStore::open(&fs, dir).unwrap();
        assert!(meta.registry_records().unwrap().is_empty());
        let puts = vec![
            (b"a".to_vec(), b"1".to_vec()),
            (b"b".to_vec(), b"2".to_vec()),
        ];
        meta.registry_apply(&puts, &[]).unwrap();
        meta.registry_apply(&[(b"c".to_vec(), b"3".to_vec())], &[b"a".to_vec()])
            .unwrap();
        meta.close().unwrap();
        let meta = MetaStore::open(&fs, dir).unwrap();
        assert_eq!(
            meta.registry_records().unwrap(),
            vec![
                (b"b".to_vec(), b"2".to_vec()),
                (b"c".to_vec(), b"3".to_vec())
            ]
        );
    }
}
