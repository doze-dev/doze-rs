//! The delegation tokens, kept for the next start (Kafka keeps them in its metadata log). The broker holds the ones in force
//! in memory and changes them only after the change here has committed.

use redb::{ReadableDatabase, ReadableTable, TableError};

use super::{MetaStore, TOKENS, lock};
use crate::MetaError;
use crate::token::TokenRecord;

impl MetaStore {
    /// Every token, by id.
    ///
    /// # Errors
    /// The database cannot be read, or holds a record this version cannot read.
    pub fn tokens(&self) -> Result<Vec<(String, TokenRecord)>, MetaError> {
        let table = match self.db.begin_read()?.open_table(TOKENS) {
            Ok(table) => table,
            // A database made before tokens could be kept.
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut all = Vec::new();
        for entry in table.iter()? {
            let (k, v) = entry?;
            all.push((k.value().to_owned(), TokenRecord::decode(v.value())?));
        }
        Ok(all)
    }

    /// Stores `puts` (each token whole, replacing what it was) and removes `removals`, in one transaction that is durable
    /// when it returns.
    ///
    /// # Errors
    /// A failed write; nothing is then changed.
    pub fn alter_tokens(
        &self,
        puts: &[(&str, &TokenRecord)],
        removals: &[&str],
    ) -> Result<(), MetaError> {
        let _one_at_a_time = lock(&self.write);
        let rows = puts
            .iter()
            .map(|(id, record)| Ok((*id, record.encode()?)))
            .collect::<Result<Vec<_>, MetaError>>()?;
        let txn = self.begin()?;
        {
            let mut table = txn.open_table(TOKENS)?;
            for id in removals {
                table.remove(*id)?;
            }
            for (id, value) in &rows {
                table.insert(*id, value.as_slice())?;
            }
        }
        txn.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use kafka_storage::fs::Fs;
    use kafka_storage::sim::SimFs;

    use super::*;

    #[test]
    fn tokens_survive_a_reopen_and_a_removal_is_gone_for_good() {
        let fs = SimFs::new();
        fs.create_dir(std::path::Path::new("/d")).unwrap();
        let token = |owner: &str| TokenRecord {
            owner: format!("User:{owner}"),
            requester: format!("User:{owner}"),
            renewers: vec!["User:carol".to_owned()],
            issue_ms: 1,
            max_ms: 3,
            expiry_ms: 2,
        };
        {
            let meta = MetaStore::open(&fs, std::path::Path::new("/d")).unwrap();
            assert!(meta.tokens().unwrap().is_empty());
            meta.alter_tokens(&[("t1", &token("alice")), ("t2", &token("bob"))], &[])
                .unwrap();
            meta.alter_tokens(&[], &["t2"]).unwrap();
            meta.close().unwrap();
        }
        let meta = MetaStore::open(&fs, std::path::Path::new("/d")).unwrap();
        assert_eq!(
            meta.tokens().unwrap(),
            vec![("t1".to_owned(), token("alice"))]
        );
    }
}
