//! The SCRAM credentials, kept for the next start. The broker holds the ones it logs users in with in memory and
//! changes them only after the change here has committed.

use redb::{ReadableDatabase, ReadableTable, TableError};

use super::{CREDENTIALS, MetaStore, lock};
use crate::MetaError;
use crate::credential::{CredentialId, CredentialRecord};

impl MetaStore {
    /// Every credential, in no particular order.
    ///
    /// # Errors
    /// The database cannot be read, or holds a record this version cannot read.
    pub fn credentials(&self) -> Result<Vec<(CredentialId, CredentialRecord)>, MetaError> {
        let table = match self.db.begin_read()?.open_table(CREDENTIALS) {
            Ok(table) => table,
            // A database made before credentials could be kept.
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut all = Vec::new();
        for entry in table.iter()? {
            let (k, v) = entry?;
            all.push((
                CredentialId::from_key(k.value())?,
                CredentialRecord::decode(v.value())?,
            ));
        }
        Ok(all)
    }

    /// Stores `credentials`, replacing the ones of the same user and mechanism, in one transaction that is durable
    /// when it returns.
    ///
    /// # Errors
    /// A failed write; none of them is then stored.
    pub fn put_credentials(
        &self,
        credentials: &[(CredentialId, CredentialRecord)],
    ) -> Result<(), MetaError> {
        self.alter_credentials(credentials, &[])
    }

    /// Removes `ids`, in one transaction that is durable when it returns. One that is not there is already gone.
    ///
    /// # Errors
    /// A failed write; none of them is then removed.
    pub fn remove_credentials(&self, ids: &[CredentialId]) -> Result<(), MetaError> {
        self.alter_credentials(&[], ids)
    }

    /// Removes `removals` and stores `puts` in one transaction that is durable when it returns: a request that
    /// deletes one user's credential and sets another's is done whole or not at all.
    ///
    /// # Errors
    /// A failed write; nothing is then changed.
    pub fn alter_credentials(
        &self,
        puts: &[(CredentialId, CredentialRecord)],
        removals: &[CredentialId],
    ) -> Result<(), MetaError> {
        let _one_at_a_time = lock(&self.write);
        let rows = puts
            .iter()
            .map(|(id, record)| Ok((id.key(), record.encode()?)))
            .collect::<Result<Vec<_>, MetaError>>()?;
        let txn = self.begin()?;
        {
            let mut table = txn.open_table(CREDENTIALS)?;
            for id in removals {
                table.remove(id.key().as_slice())?;
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
mod tests;
