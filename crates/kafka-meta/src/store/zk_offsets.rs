//! The offsets OffsetCommit v0 commits, kept apart from every other offset, as Kafka 1.1 and 2.8 keep them in ZooKeeper
//! (`/consumers/<group>/offsets/<topic>/<partition>`): only the offset, no metadata, no member or generation, and read
//! back only by OffsetFetch v0. A KRaft broker (Kafka 3) has no such place, and the broker does not ask for it there.
//!
//! The key is the committed offsets' (group, NUL, topic, NUL, partition); the value the offset, eight bytes big-endian.
//! The table is absent from a database made before it, which reads take as none and the first write makes.

use kafka_wire::ids::{GroupId, PartitionId, TopicName};
use redb::{ReadableDatabase, TableDefinition, TableError};

use super::{MetaStore, lock};
use crate::MetaError;
use crate::offsets::Committed;

const ZK_OFFSETS: TableDefinition<'static, &[u8], u64> = TableDefinition::new("zk_offsets");

fn key(group: &GroupId, topic: &TopicName, partition: PartitionId) -> Vec<u8> {
    let mut k = group.as_str().as_bytes().to_vec();
    k.push(0);
    k.extend_from_slice(topic.as_str().as_bytes());
    k.push(0);
    k.extend_from_slice(&partition.get().to_be_bytes());
    k
}

impl MetaStore {
    /// Stores what an OffsetCommit v0 commits for `group`, durably and in one write. A topic or partition that does not
    /// exist is not stored and is told so.
    ///
    /// # Errors
    /// A failed write, in which case nothing was stored.
    pub fn commit_zk_offsets(
        &self,
        group: &GroupId,
        commits: &[(TopicName, PartitionId, i64)],
    ) -> Result<Vec<Committed>, MetaError> {
        let _one_at_a_time = lock(&self.write);
        let outcomes: Vec<Committed> = commits
            .iter()
            .map(|(topic, partition, _)| match self.topic(topic) {
                Some(t) if partition.get() < t.partitions.get() => Committed::Stored,
                _ => Committed::UnknownTopicOrPartition,
            })
            .collect();
        if !outcomes.contains(&Committed::Stored) {
            return Ok(outcomes);
        }
        let txn = self.begin()?;
        {
            let mut table = txn.open_table(ZK_OFFSETS)?;
            for ((topic, partition, offset), outcome) in commits.iter().zip(&outcomes) {
                if *outcome == Committed::Stored {
                    table.insert(key(group, topic, *partition).as_slice(), offset.cast_unsigned())?;
                }
            }
        }
        txn.commit()?;
        Ok(outcomes)
    }

    /// What OffsetCommit v0 last stored for one partition of `group`.
    ///
    /// # Errors
    /// The database cannot be read.
    pub fn zk_offset(
        &self,
        group: &GroupId,
        topic: &TopicName,
        partition: PartitionId,
    ) -> Result<Option<i64>, MetaError> {
        let table = match self.db.begin_read()?.open_table(ZK_OFFSETS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        Ok(table
            .get(key(group, topic, partition).as_slice())?
            .map(|v| v.value().cast_signed()))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};
    use std::path::Path;

    use kafka_storage::fs::Fs;
    use kafka_storage::sim::SimFs;
    use kafka_wire::ids::PartitionCount;

    use super::*;

    #[test]
    fn a_v0_commit_is_kept_apart_and_read_back_only_from_its_own_place() {
        let fs = SimFs::new();
        fs.create_dir(Path::new("/data")).unwrap();
        fs.sync_dir(Path::new("/")).unwrap();
        let store = MetaStore::open(&fs, Path::new("/data")).unwrap();
        let name = TopicName::parse("orders").unwrap();
        store
            .create_topic(&name, PartitionCount::new(2).unwrap(), BTreeMap::new(), &HashSet::new())
            .unwrap();
        let group = GroupId::parse("old").unwrap();
        let p = |n| PartitionId::new(n).unwrap();
        let outcomes = store
            .commit_zk_offsets(&group, &[(name.clone(), p(1), 42), (name.clone(), p(5), 1)])
            .unwrap();
        assert_eq!(outcomes, [Committed::Stored, Committed::UnknownTopicOrPartition]);
        assert_eq!(store.zk_offset(&group, &name, p(1)).unwrap(), Some(42));
        assert_eq!(store.zk_offset(&group, &name, p(0)).unwrap(), None);
        // Not an offset the group has committed by any later version.
        assert_eq!(store.offset(&group, &name, p(1)).unwrap(), None);
        drop(store);
        let again = MetaStore::open(&fs, Path::new("/data")).unwrap();
        assert_eq!(again.zk_offset(&group, &name, p(1)).unwrap(), Some(42), "kept across a restart");
    }
}
