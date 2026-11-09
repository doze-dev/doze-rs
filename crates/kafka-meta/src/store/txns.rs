//! The records of transactional ids.

use redb::{ReadableDatabase, ReadableTable, TableError};

use super::{MetaStore, TXNS, lock};
use crate::MetaError;
use crate::txn::TxnRecord;

impl MetaStore {
    /// Records a transactional id's state, and makes it durable. The change is applied by the caller only if this
    /// succeeds, so that what is in memory never says more than the store does.
    ///
    /// # Errors
    /// A failed write; the record is then as it was.
    pub fn put_txn(&self, record: &TxnRecord) -> Result<(), MetaError> {
        let _one_at_a_time = lock(&self.write);
        let bytes = record.encode()?;
        let txn = self.begin()?;
        {
            txn.open_table(TXNS)?
                .insert(record.id.as_str(), bytes.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    /// Every transactional id's record, by id.
    ///
    /// # Errors
    /// The database cannot be read, or holds a record this version cannot read.
    pub fn txns(&self) -> Result<Vec<TxnRecord>, MetaError> {
        let table = match self.db.begin_read()?.open_table(TXNS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut all = Vec::new();
        for entry in table.iter()? {
            let (_, v) = entry?;
            all.push(TxnRecord::decode(v.value())?);
        }
        Ok(all)
    }
}
