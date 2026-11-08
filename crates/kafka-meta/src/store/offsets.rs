//! Committed offsets: a group's progress through each partition.
//!
//! The key is the group, a NUL, the topic, a NUL and the partition as four big-endian bytes, so everything of one
//! group lies together in order and a range finds it. Neither a group id nor a topic name can hold a NUL.

use kafka_wire::ids::{GroupId, PartitionId, TopicName};
use redb::{ReadableDatabase, ReadableTable, TableError, WriteTransaction};

use super::{MetaStore, OFFSETS, lock};
use crate::MetaError;
use crate::offsets::{Committed, GroupOffset, OffsetCommit, StoredOffset};

type OffsetsTable = redb::ReadOnlyTable<&'static [u8], &'static [u8]>;

fn key(group: &GroupId, topic: &TopicName, partition: PartitionId) -> Vec<u8> {
    let mut k = prefix(group);
    k.extend_from_slice(topic.as_str().as_bytes());
    k.push(0);
    k.extend_from_slice(&partition.get().to_be_bytes());
    k
}

/// The start of everything a group has.
fn prefix(group: &GroupId) -> Vec<u8> {
    let mut k = group.as_str().as_bytes().to_vec();
    k.push(0);
    k
}

/// The first key past everything a group has.
fn past(group: &GroupId) -> Vec<u8> {
    let mut k = group.as_str().as_bytes().to_vec();
    k.push(1);
    k
}

/// A key's topic and partition, and its group's text.
fn split(key: &[u8]) -> Result<(&str, TopicName, PartitionId), MetaError> {
    let corrupt = || MetaError::Corrupt("an offset's key is malformed".into());
    let mut parts = key.splitn(3, |b| *b == 0);
    let (group, topic, rest) = (parts.next(), parts.next(), parts.next());
    let (Some(group), Some(topic), Some(rest)) = (group, topic, rest) else {
        return Err(corrupt());
    };
    let partition = <[u8; 4]>::try_from(rest)
        .ok()
        .and_then(|b| PartitionId::new(i32::from_be_bytes(b)))
        .ok_or_else(corrupt)?;
    Ok((
        std::str::from_utf8(group).map_err(|_| corrupt())?,
        TopicName::parse(std::str::from_utf8(topic).map_err(|_| corrupt())?)
            .map_err(|_| corrupt())?,
        partition,
    ))
}

/// Removes every offset of `topic`, in the transaction that deletes the topic, so that a topic made again under
/// the same name does not meet offsets from its past.
pub(super) fn remove_topic(txn: &WriteTransaction, topic: &TopicName) -> Result<(), MetaError> {
    let mut table = txn.open_table(OFFSETS)?;
    let mut doomed: Vec<Vec<u8>> = Vec::new();
    for entry in table.iter()? {
        let (k, _) = entry?;
        if split(k.value())?.1 == *topic {
            doomed.push(k.value().to_vec());
        }
    }
    for k in doomed {
        table.remove(k.as_slice())?;
    }
    Ok(())
}

impl MetaStore {
    /// Commits offsets for `group` and makes them durable, all in one write. A commit to a topic or partition
    /// that does not exist is not stored and is told so; the check is made under the lock that topic changes
    /// take, so a commit cannot land after its topic was deleted.
    ///
    /// # Errors
    /// A failed write, in which case nothing was stored.
    pub fn commit_offsets(
        &self,
        group: &GroupId,
        commits: &[OffsetCommit],
    ) -> Result<Vec<Committed>, MetaError> {
        let _one_at_a_time = lock(&self.write);
        let outcomes: Vec<Committed> = commits
            .iter()
            .map(|c| match self.topic(&c.topic) {
                Some(t) if c.partition.get() < t.partitions.get() => Committed::Stored,
                _ => Committed::UnknownTopicOrPartition,
            })
            .collect();
        if !outcomes.contains(&Committed::Stored) {
            return Ok(outcomes);
        }
        let txn = self.begin()?;
        {
            let mut table = txn.open_table(OFFSETS)?;
            for (c, outcome) in commits.iter().zip(&outcomes) {
                if *outcome == Committed::Stored {
                    let record = c.offset.encode()?;
                    table.insert(
                        key(group, &c.topic, c.partition).as_slice(),
                        record.as_slice(),
                    )?;
                }
            }
        }
        txn.commit()?;
        Ok(outcomes)
    }

    /// What `group` has committed for one partition.
    ///
    /// # Errors
    /// The database cannot be read.
    pub fn offset(
        &self,
        group: &GroupId,
        topic: &TopicName,
        partition: PartitionId,
    ) -> Result<Option<StoredOffset>, MetaError> {
        let Some(table) = self.offsets_table()? else {
            return Ok(None);
        };
        let k = key(group, topic, partition);
        table
            .get(k.as_slice())?
            .map(|v| StoredOffset::decode(v.value()))
            .transpose()
    }

    /// Everything `group` has committed, by topic and partition.
    ///
    /// # Errors
    /// The database cannot be read.
    pub fn group_offsets(&self, group: &GroupId) -> Result<Vec<GroupOffset>, MetaError> {
        let Some(table) = self.offsets_table()? else {
            return Ok(Vec::new());
        };
        let (from, to) = (prefix(group), past(group));
        let mut all = Vec::new();
        for entry in table.range(from.as_slice()..to.as_slice())? {
            let (k, v) = entry?;
            let (_, topic, partition) = split(k.value())?;
            all.push(GroupOffset {
                topic,
                partition,
                offset: StoredOffset::decode(v.value())?,
            });
        }
        // The key orders by the topic's bytes and then the partition: the order a client is answered in.
        Ok(all)
    }

    /// The groups that have committed anything.
    ///
    /// # Errors
    /// The database cannot be read.
    pub fn groups_with_offsets(&self) -> Result<Vec<GroupId>, MetaError> {
        let Some(table) = self.offsets_table()? else {
            return Ok(Vec::new());
        };
        let mut groups: Vec<GroupId> = Vec::new();
        for entry in table.iter()? {
            let (k, _) = entry?;
            let (group, _, _) = split(k.value())?;
            if groups.last().is_none_or(|g| g.as_str() != group) {
                groups
                    .push(GroupId::parse(group).map_err(|_| {
                        MetaError::Corrupt("an offset's group is malformed".into())
                    })?);
            }
        }
        Ok(groups)
    }

    /// Forgets the offsets of `group` for the given partitions; returns how many there were.
    ///
    /// # Errors
    /// A failed write, in which case none was forgotten.
    pub fn delete_offsets(
        &self,
        group: &GroupId,
        which: &[(TopicName, PartitionId)],
    ) -> Result<usize, MetaError> {
        let _one_at_a_time = lock(&self.write);
        let txn = self.begin()?;
        let mut removed = 0;
        {
            let mut table = txn.open_table(OFFSETS)?;
            for (topic, partition) in which {
                let k = key(group, topic, *partition);
                removed += usize::from(table.remove(k.as_slice())?.is_some());
            }
        }
        if removed == 0 {
            return Ok(0);
        }
        txn.commit()?;
        Ok(removed)
    }

    /// Forgets every offset of `group`; returns how many there were.
    ///
    /// # Errors
    /// A failed write, in which case none was forgotten.
    pub fn delete_group_offsets(&self, group: &GroupId) -> Result<usize, MetaError> {
        let _one_at_a_time = lock(&self.write);
        let txn = self.begin()?;
        let removed = {
            let mut table = txn.open_table(OFFSETS)?;
            let (from, to) = (prefix(group), past(group));
            let keys: Vec<Vec<u8>> = table
                .range(from.as_slice()..to.as_slice())?
                .map(|e| e.map(|(k, _)| k.value().to_vec()))
                .collect::<Result<_, _>>()?;
            for k in &keys {
                table.remove(k.as_slice())?;
            }
            keys.len()
        };
        if removed == 0 {
            return Ok(0);
        }
        txn.commit()?;
        Ok(removed)
    }

    /// The offsets table, or `None` if no offset has ever been committed (a database made before the table was).
    fn offsets_table(&self) -> Result<Option<OffsetsTable>, MetaError> {
        match self.db.begin_read()?.open_table(OFFSETS) {
            Ok(table) => Ok(Some(table)),
            Err(TableError::TableDoesNotExist(_)) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests;
