//! The group records kept for the offsets retention.

use kafka_wire::ids::GroupId;
use redb::{ReadableDatabase, ReadableTable, TableError};

use super::{GROUP_LIVES, MetaStore, lock};
use crate::MetaError;
use crate::life::GroupLife;

impl MetaStore {
    /// Records what is known of a group's members, and makes it durable.
    ///
    /// # Errors
    /// A failed write; the record is then as it was.
    pub fn set_group_life(&self, group: &GroupId, life: &GroupLife) -> Result<(), MetaError> {
        let _one_at_a_time = lock(&self.write);
        let record = life.encode()?;
        let txn = self.begin()?;
        {
            txn.open_table(GROUP_LIVES)?
                .insert(group.as_str(), record.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    /// Forgets what is known of a group.
    ///
    /// # Errors
    /// A failed write; the record is then as it was.
    pub fn forget_group_life(&self, group: &GroupId) -> Result<(), MetaError> {
        let _one_at_a_time = lock(&self.write);
        let txn = self.begin()?;
        let had = {
            let mut table = txn.open_table(GROUP_LIVES)?;
            table.remove(group.as_str())?.is_some()
        };
        if had {
            txn.commit()?;
        }
        Ok(())
    }

    /// What is recorded of one group.
    ///
    /// # Errors
    /// The database cannot be read.
    pub fn group_life(&self, group: &GroupId) -> Result<Option<GroupLife>, MetaError> {
        Ok(self
            .group_lives()?
            .into_iter()
            .find(|(g, _)| g == group)
            .map(|(_, life)| life))
    }

    /// What is recorded of every group, by id.
    ///
    /// # Errors
    /// The database cannot be read, or holds a record this version cannot read.
    pub fn group_lives(&self) -> Result<Vec<(GroupId, GroupLife)>, MetaError> {
        let table = match self.db.begin_read()?.open_table(GROUP_LIVES) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut all = Vec::new();
        for entry in table.iter()? {
            let (k, v) = entry?;
            let group = GroupId::parse(k.value())
                .map_err(|_| MetaError::Corrupt("a group record's id is malformed".into()))?;
            all.push((group, GroupLife::decode(v.value())?));
        }
        Ok(all)
    }
}

#[cfg(test)]
mod tests;
