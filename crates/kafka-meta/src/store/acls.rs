//! The ACLs, kept for the next start. The broker holds the set it enforces in memory and changes it only after
//! the change here has committed.

use redb::{ReadableDatabase, ReadableTable, TableError};

use super::{ACLS, MetaStore, lock};
use crate::MetaError;
use crate::acl::AclRecord;

impl MetaStore {
    /// Every ACL, in no particular order.
    ///
    /// # Errors
    /// The database cannot be read, or holds a record this version cannot read.
    pub fn acls(&self) -> Result<Vec<AclRecord>, MetaError> {
        let table = match self.db.begin_read()?.open_table(ACLS) {
            Ok(table) => table,
            // A database made before ACLs existed.
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut all = Vec::new();
        for entry in table.iter()? {
            let (_, v) = entry?;
            all.push(AclRecord::decode(v.value())?);
        }
        Ok(all)
    }

    /// Stores `acls`, in one transaction that is durable when it returns. One that is stored already is left as it
    /// is: the same seven fields are the same ACL.
    ///
    /// # Errors
    /// A failed write; none of them is then stored.
    pub fn add_acls(&self, acls: &[AclRecord]) -> Result<(), MetaError> {
        let _one_at_a_time = lock(&self.write);
        let rows = acls
            .iter()
            .map(|a| Ok((a.key()?, a.encode()?)))
            .collect::<Result<Vec<_>, MetaError>>()?;
        let txn = self.begin()?;
        {
            let mut table = txn.open_table(ACLS)?;
            for (key, value) in &rows {
                table.insert(key.as_slice(), value.as_slice())?;
            }
        }
        txn.commit()?;
        Ok(())
    }

    /// Removes `acls`, in one transaction that is durable when it returns. One that is not there is already gone.
    ///
    /// # Errors
    /// A failed write; none of them is then removed.
    pub fn remove_acls(&self, acls: &[AclRecord]) -> Result<(), MetaError> {
        let _one_at_a_time = lock(&self.write);
        let keys = acls
            .iter()
            .map(AclRecord::key)
            .collect::<Result<Vec<_>, _>>()?;
        let txn = self.begin()?;
        {
            let mut table = txn.open_table(ACLS)?;
            for key in &keys {
                table.remove(key.as_slice())?;
            }
        }
        txn.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
