//! The client quotas, kept for the next start. The broker holds the ones it enforces in memory and changes them only after
//! the change here has committed.

use redb::{ReadableDatabase, ReadableTable, TableError};

use super::{MetaStore, QUOTAS, lock};
use crate::MetaError;
use crate::quota::{QuotaEntity, QuotaValues};

impl MetaStore {
    /// Every entity with a quota, and its values, in no particular order.
    ///
    /// # Errors
    /// The database cannot be read, or holds a record this version cannot read.
    pub fn quotas(&self) -> Result<Vec<(QuotaEntity, QuotaValues)>, MetaError> {
        let table = match self.db.begin_read()?.open_table(QUOTAS) {
            Ok(table) => table,
            // A database made before quotas could be kept.
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut all = Vec::new();
        for entry in table.iter()? {
            let (k, v) = entry?;
            all.push((
                QuotaEntity::from_key(k.value())?,
                QuotaValues::decode(v.value())?,
            ));
        }
        Ok(all)
    }

    /// Stores `puts` (each entity's values whole, replacing what it had) and removes `removals`, in one transaction that is
    /// durable when it returns: an AlterClientQuotas is done whole or not at all.
    ///
    /// # Errors
    /// A failed write; nothing is then changed.
    pub fn alter_quotas(
        &self,
        puts: &[(QuotaEntity, QuotaValues)],
        removals: &[QuotaEntity],
    ) -> Result<(), MetaError> {
        let _one_at_a_time = lock(&self.write);
        let rows = puts
            .iter()
            .map(|(entity, values)| Ok((entity.key()?, values.encode()?)))
            .collect::<Result<Vec<_>, MetaError>>()?;
        let gone = removals
            .iter()
            .map(QuotaEntity::key)
            .collect::<Result<Vec<_>, MetaError>>()?;
        let txn = self.begin()?;
        {
            let mut table = txn.open_table(QUOTAS)?;
            for key in &gone {
                table.remove(key.as_slice())?;
            }
            for (key, value) in &rows {
                table.insert(key.as_slice(), value.as_slice())?;
            }
        }
        txn.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use kafka_storage::fs::Fs;
    use kafka_storage::sim::SimFs;

    use super::*;
    use crate::quota::QuotaName;

    fn open(fs: &SimFs) -> MetaStore {
        MetaStore::open(fs, std::path::Path::new("/d")).unwrap()
    }

    #[test]
    fn quotas_survive_a_reopen_and_a_removal_is_gone_for_good() {
        let fs = SimFs::new();
        fs.create_dir(std::path::Path::new("/d")).unwrap();
        let alice = QuotaEntity {
            user: Some(QuotaName::Named("alice".into())),
            client_id: None,
            ip: None,
        };
        let clients = QuotaEntity {
            user: None,
            client_id: Some(QuotaName::Default),
            ip: None,
        };
        let values = |k: &str, v: f64| QuotaValues(BTreeMap::from([(k.to_owned(), v)]));
        {
            let meta = open(&fs);
            assert!(meta.quotas().unwrap().is_empty());
            meta.alter_quotas(
                &[
                    (alice.clone(), values("producer_byte_rate", 1024.0)),
                    (clients.clone(), values("request_percentage", 50.0)),
                ],
                &[],
            )
            .unwrap();
            meta.alter_quotas(&[], &[clients]).unwrap();
            meta.close().unwrap();
        }
        let meta = open(&fs);
        assert_eq!(
            meta.quotas().unwrap(),
            vec![(alice, values("producer_byte_rate", 1024.0))]
        );
    }
}
