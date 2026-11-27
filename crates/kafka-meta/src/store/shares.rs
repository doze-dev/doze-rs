//! The share groups' partition states (KIP-932): what Kafka's share coordinator keeps in `__share_group_state`.
//!
//! The key is the one committed offsets have (the group, a NUL, the topic, a NUL, the partition as four big-endian bytes),
//! so a group's states lie together and a range finds them, and a deleted topic's go in the transaction that deletes it.

use kafka_wire::ids::{GroupId, PartitionId, TopicName};
use redb::{ReadableDatabase, ReadableTable, TableError, WriteTransaction};

use super::together::{Applied, Txn, Write};
use super::{MetaStore, SHARE_STATES, lock};
use crate::MetaError;
use crate::offsets::Committed;
use crate::share::ShareState;

type StatesTable = redb::ReadOnlyTable<&'static [u8], &'static [u8]>;

fn key(group: &GroupId, topic: &TopicName, partition: PartitionId) -> Vec<u8> {
    let mut k = prefix(group);
    k.extend_from_slice(topic.as_str().as_bytes());
    k.push(0);
    k.extend_from_slice(&partition.get().to_be_bytes());
    k
}

fn prefix(group: &GroupId) -> Vec<u8> {
    let mut k = group.as_str().as_bytes().to_vec();
    k.push(0);
    k
}

fn past(group: &GroupId) -> Vec<u8> {
    let mut k = group.as_str().as_bytes().to_vec();
    k.push(1);
    k
}

/// A key's group text, topic and partition.
fn split(key: &[u8]) -> Result<(&str, TopicName, PartitionId), MetaError> {
    let corrupt = || MetaError::Corrupt("a share state's key is malformed".into());
    let mut parts = key.splitn(3, |b| *b == 0);
    let (Some(group), Some(topic), Some(rest)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(corrupt());
    };
    let partition = <[u8; 4]>::try_from(rest)
        .ok()
        .and_then(|b| PartitionId::new(i32::from_be_bytes(b)))
        .ok_or_else(corrupt)?;
    let topic = std::str::from_utf8(topic).map_err(|_| corrupt())?;
    Ok((
        std::str::from_utf8(group).map_err(|_| corrupt())?,
        TopicName::parse(topic).map_err(|_| corrupt())?,
        partition,
    ))
}

/// One partition's state, as a write names it.
pub type ShareStateWrite<'a> = (&'a TopicName, PartitionId, &'a ShareState);

/// Removes every share state of `topic`, in the transaction that deletes the topic: a topic made again under the same
/// name starts with no share-partition (Kafka keys the state by topic id, which a new topic does not share).
pub(super) fn remove_topic(txn: &WriteTransaction, topic: &TopicName) -> Result<(), MetaError> {
    let mut table = txn.open_table(SHARE_STATES)?;
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
    /// Stores the states of a group's partitions, all in one durable write. A partition whose topic is gone (or that
    /// is past the topic's partitions) is not stored: checked under the lock topic changes take, so a state cannot land
    /// after its topic was deleted. Returns, for each, whether it was stored.
    ///
    /// Writes that arrive together (the acknowledgements of other share-partitions, offset commits) share one
    /// transaction and one flush (`store/together.rs`); each is answered once its own is flushed.
    ///
    /// # Errors
    /// A failed write, in which case nothing was stored.
    pub fn set_share_states(
        &self,
        group: &GroupId,
        states: &[ShareStateWrite<'_>],
    ) -> Result<Vec<bool>, MetaError> {
        let states = states
            .iter()
            .map(|(topic, partition, state)| ((*topic).clone(), *partition, (*state).clone()))
            .collect();
        let done = self.together(Write::ShareStates {
            group: group.clone(),
            states,
        })?;
        Ok(done.iter().map(|c| *c == Committed::Stored).collect())
    }

    /// Applies one write of states in the transaction of its batch, under the lock topic changes take. Judged and
    /// encoded before anything is written, so a write refused alone writes nothing.
    pub(super) fn apply_share_states(
        &self,
        txn: &mut Txn<'_>,
        group: &GroupId,
        states: &[(TopicName, PartitionId, ShareState)],
    ) -> Applied {
        let outcomes: Vec<Committed> = states
            .iter()
            .map(|(topic, partition, _)| match self.topic(topic) {
                Some(t) if partition.get() < t.partitions.get() => Committed::Stored,
                _ => Committed::UnknownTopicOrPartition,
            })
            .collect();
        if !outcomes.contains(&Committed::Stored) {
            return Ok(Ok(outcomes));
        }
        let kept = || {
            states
                .iter()
                .zip(&outcomes)
                .filter(|(_, o)| **o == Committed::Stored)
        };
        let records: Result<Vec<Vec<u8>>, MetaError> = kept().map(|(s, _)| s.2.encode()).collect();
        let records = match records {
            Ok(r) => r,
            Err(e) => return Ok(Err(e)),
        };
        let mut table = txn.begun()?.open_table(SHARE_STATES)?;
        for (((topic, partition, _), _), record) in kept().zip(&records) {
            table.insert(key(group, topic, *partition).as_slice(), record.as_slice())?;
        }
        Ok(Ok(outcomes))
    }

    /// The state of one partition of a share group, if it has one.
    ///
    /// # Errors
    /// The database cannot be read, or holds a record this version cannot read.
    pub fn share_state(
        &self,
        group: &GroupId,
        topic: &TopicName,
        partition: PartitionId,
    ) -> Result<Option<ShareState>, MetaError> {
        let Some(table) = self.states_table()? else {
            return Ok(None);
        };
        table
            .get(key(group, topic, partition).as_slice())?
            .map(|v| ShareState::decode(v.value()))
            .transpose()
    }

    /// Every partition state of a share group, by topic and partition.
    ///
    /// # Errors
    /// The database cannot be read, or holds a record this version cannot read.
    pub fn share_states(
        &self,
        group: &GroupId,
    ) -> Result<Vec<(TopicName, PartitionId, ShareState)>, MetaError> {
        let Some(table) = self.states_table()? else {
            return Ok(Vec::new());
        };
        let (from, to) = (prefix(group), past(group));
        let mut all = Vec::new();
        for entry in table.range(from.as_slice()..to.as_slice())? {
            let (k, v) = entry?;
            let (_, topic, partition) = split(k.value())?;
            all.push((topic, partition, ShareState::decode(v.value())?));
        }
        Ok(all)
    }

    /// Forgets a share group's partition states: of the topics named, or of every topic. Returns how many went.
    ///
    /// # Errors
    /// A failed write, in which case none was forgotten.
    pub fn delete_share_states(
        &self,
        group: &GroupId,
        topics: Option<&[TopicName]>,
    ) -> Result<usize, MetaError> {
        let _one_at_a_time = lock(&self.write);
        let txn = self.begin()?;
        let removed = {
            let mut table = txn.open_table(SHARE_STATES)?;
            let (from, to) = (prefix(group), past(group));
            let mut keys: Vec<Vec<u8>> = Vec::new();
            for entry in table.range(from.as_slice()..to.as_slice())? {
                let (k, _) = entry?;
                let (_, topic, _) = split(k.value())?;
                if topics.is_none_or(|named| named.contains(&topic)) {
                    keys.push(k.value().to_vec());
                }
            }
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

    /// The table, or `None` if no share state was ever stored (a database made before share groups).
    fn states_table(&self) -> Result<Option<StatesTable>, MetaError> {
        match self.db.begin_read()?.open_table(SHARE_STATES) {
            Ok(table) => Ok(Some(table)),
            Err(TableError::TableDoesNotExist(_)) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests;
