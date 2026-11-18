//! The configs set on groups (IncrementalAlterConfigs on resource type GROUP): the group, a NUL, the config name, to the
//! value as text. A group need not exist to have configs (Kafka keeps them in the cluster's metadata, apart from the group).

use std::collections::BTreeMap;

use kafka_wire::ids::GroupId;
use redb::{ReadableDatabase, ReadableTable, TableError};

use super::{GROUP_CONFIGS, MetaStore, lock};
use crate::MetaError;

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

impl MetaStore {
    /// Replaces the configs of `group` with `configs` (none: the group has none), in one durable write.
    ///
    /// # Errors
    /// A failed write, in which case the configs are as they were.
    pub fn set_group_configs(
        &self,
        group: &GroupId,
        configs: &BTreeMap<String, String>,
    ) -> Result<(), MetaError> {
        let _one_at_a_time = lock(&self.write);
        let txn = self.begin()?;
        {
            let mut table = txn.open_table(GROUP_CONFIGS)?;
            let (from, to) = (prefix(group), past(group));
            let old: Vec<Vec<u8>> = table
                .range(from.as_slice()..to.as_slice())?
                .map(|e| e.map(|(k, _)| k.value().to_vec()))
                .collect::<Result<_, _>>()?;
            for k in &old {
                table.remove(k.as_slice())?;
            }
            for (name, value) in configs {
                let mut k = prefix(group);
                k.extend_from_slice(name.as_bytes());
                table.insert(k.as_slice(), value.as_bytes())?;
            }
        }
        txn.commit()?;
        Ok(())
    }

    /// Every group's configs.
    ///
    /// # Errors
    /// The database cannot be read, or holds a key this version cannot read.
    pub fn group_configs(&self) -> Result<BTreeMap<GroupId, BTreeMap<String, String>>, MetaError> {
        let table = match self.db.begin_read()?.open_table(GROUP_CONFIGS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(BTreeMap::new()),
            Err(e) => return Err(e.into()),
        };
        let corrupt = || MetaError::Corrupt("a group config's key is malformed".into());
        let mut all: BTreeMap<GroupId, BTreeMap<String, String>> = BTreeMap::new();
        for entry in table.iter()? {
            let (k, v) = entry?;
            let mut parts = k.value().splitn(2, |b| *b == 0);
            let (Some(group), Some(name)) = (parts.next(), parts.next()) else {
                return Err(corrupt());
            };
            let group = std::str::from_utf8(group)
                .ok()
                .and_then(|g| GroupId::parse(g).ok())
                .ok_or_else(corrupt)?;
            let name = String::from_utf8(name.to_vec()).map_err(|_| corrupt())?;
            let value = String::from_utf8(v.value().to_vec()).map_err(|_| corrupt())?;
            all.entry(group).or_default().insert(name, value);
        }
        Ok(all)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use kafka_storage::fs::Fs;
    use kafka_storage::sim::{Cut, DataCut, NameCut, SimFs};

    use super::*;

    #[test]
    fn a_groups_configs_are_replaced_whole_kept_apart_from_others_and_survive_the_power_going() {
        let fs = SimFs::new();
        fs.create_dir(Path::new("/data")).unwrap();
        fs.sync_dir(Path::new("/")).unwrap();
        let store = MetaStore::open(&fs, Path::new("/data")).unwrap();
        let (g, g2) = (GroupId::parse("g").unwrap(), GroupId::parse("g2").unwrap());
        let one = BTreeMap::from([("share.delivery.count.limit".to_owned(), "3".to_owned())]);
        store.set_group_configs(&g, &one).unwrap();
        store.set_group_configs(&g2, &one).unwrap();
        let two = BTreeMap::from([("share.auto.offset.reset".to_owned(), "earliest".to_owned())]);
        store.set_group_configs(&g, &two).unwrap();
        drop(store);
        let cut = Cut {
            data: DataCut::Drop,
            names: NameCut::Durable,
            seed: 3,
        };
        let store = MetaStore::open(&fs.power_cut(cut), Path::new("/data")).unwrap();
        let all = store.group_configs().unwrap();
        assert_eq!(all.get(&g), Some(&two));
        assert_eq!(all.get(&g2), Some(&one));
        store.set_group_configs(&g, &BTreeMap::new()).unwrap();
        assert!(!store.group_configs().unwrap().contains_key(&g));
    }
}
