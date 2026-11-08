//! Committed offsets in the workload: commits, and the check that what is stored is what was told.

use std::collections::{HashMap, HashSet};

use kafka_meta::{MetaStore, OffsetCommit, StoredOffset};
use kafka_wire::ids::{GroupId, PartitionId, TopicName};

use super::{GROUPS, Model, World};

/// A committed offset's place: group, topic, partition.
pub(super) type Place = (&'static str, TopicName, i32);

/// The offsets the workload has been told of.
#[derive(Default)]
pub(crate) struct Offsets {
    /// Committed and acknowledged, and not removed with their topic since.
    pub(crate) acked: HashMap<Place, i64>,
    /// What commits that failed part-way may have left in a place.
    pub(crate) maybe: HashMap<Place, HashSet<i64>>,
    /// Places in `acked` that a delete which failed part-way may have removed.
    pub(crate) maybe_gone: HashSet<Place>,
}

impl Model {
    pub(super) fn check_offsets(&self, store: &MetaStore) -> Result<(), String> {
        let gid = |g: &str| GroupId::parse(g).unwrap();
        for ((group, topic, partition), want) in &self.offsets.acked {
            let got = store
                .offset(&gid(group), topic, PartitionId::new(*partition).unwrap())
                .map_err(|e| e.to_string())?;
            let allowed = |v: i64| {
                v == *want
                    || self
                        .offsets
                        .maybe
                        .get(&(group, topic.clone(), *partition))
                        .is_some_and(|m| m.contains(&v))
            };
            match got {
                Some(o) if allowed(o.offset) => {}
                Some(o) => {
                    return Err(format!(
                        "{group}/{topic}/{partition} came back as {}, was {want}",
                        o.offset
                    ));
                }
                None if self.maybe_deleted.contains(topic)
                    || self
                        .offsets
                        .maybe_gone
                        .contains(&(group, topic.clone(), *partition)) => {}
                None => {
                    return Err(format!(
                        "{group}/{topic}/{partition} was acknowledged and is gone"
                    ));
                }
            }
        }
        for group in store.groups_with_offsets().map_err(|e| e.to_string())? {
            for found in store.group_offsets(&group).map_err(|e| e.to_string())? {
                let place = (
                    GROUPS
                        .iter()
                        .find(|g| **g == group.as_str())
                        .copied()
                        .unwrap_or("?"),
                    found.topic.clone(),
                    found.partition.get(),
                );
                if store.topic(&found.topic).is_none() {
                    return Err(format!(
                        "an offset of {group} outlived its topic {}",
                        found.topic
                    ));
                }
                let known = self.offsets.acked.get(&place) == Some(&found.offset.offset)
                    || self
                        .offsets
                        .maybe
                        .get(&place)
                        .is_some_and(|m| m.contains(&found.offset.offset));
                if !known {
                    return Err(format!(
                        "{group}/{}/{} holds {}, which was never committed there",
                        found.topic, found.partition, found.offset.offset
                    ));
                }
            }
        }
        Ok(())
    }
}

impl World {
    pub(super) fn commit(&mut self) -> &'static str {
        let mut topics: Vec<_> = self.model.acked.values().cloned().collect();
        topics.sort_by(|a, b| a.name.cmp(&b.name));
        if topics.is_empty() || self.store.is_none() {
            return "commit";
        }
        let topic = &topics[self.rng.below(topics.len() as u64) as usize];
        let group = GROUPS[self.rng.below(GROUPS.len() as u64) as usize];
        let partition = self.rng.below(topic.partitions.get() as u64) as i32;
        let value = self.rng.below(1000) as i64;
        let commit = OffsetCommit {
            topic: topic.name.clone(),
            partition: PartitionId::new(partition).unwrap(),
            offset: StoredOffset {
                offset: value,
                leader_epoch: -1,
                metadata: String::new(),
                committed_ms: 0,
            },
        };
        let place = (group, topic.name.clone(), partition);
        let store = self.store.as_ref().unwrap();
        if store
            .commit_offsets(&GroupId::parse(group).unwrap(), &[commit])
            .is_ok()
        {
            self.model.offsets.maybe_gone.remove(&place);
            self.model.offsets.acked.insert(place, value);
        } else {
            self.model
                .offsets
                .maybe
                .entry(place)
                .or_default()
                .insert(value);
            self.store = None;
        }
        "commit"
    }

    /// Forgets one offset the workload knows of.
    pub(super) fn delete_offsets(&mut self) -> &'static str {
        let mut places: Vec<Place> = self.model.offsets.acked.keys().cloned().collect();
        places.sort();
        let Some(store) = &self.store else {
            return "delete_offsets";
        };
        if places.is_empty() {
            return "delete_offsets";
        }
        let place = places[self.rng.below(places.len() as u64) as usize].clone();
        let which = [(place.1.clone(), PartitionId::new(place.2).unwrap())];
        match store.delete_offsets(&GroupId::parse(place.0).unwrap(), &which) {
            Ok(_) => {
                self.model.offsets.acked.remove(&place);
                self.model.offsets.maybe.remove(&place);
                self.model.offsets.maybe_gone.remove(&place);
            }
            Err(_) => {
                self.model.offsets.maybe_gone.insert(place);
                self.store = None;
            }
        }
        "delete_offsets"
    }

    /// Forgets everything a group has committed.
    pub(super) fn delete_group_offsets(&mut self) -> &'static str {
        let group = GROUPS[self.rng.below(GROUPS.len() as u64) as usize];
        let Some(store) = &self.store else {
            return "delete_group_offsets";
        };
        let offsets = &mut self.model.offsets;
        match store.delete_group_offsets(&GroupId::parse(group).unwrap()) {
            Ok(_) => {
                offsets.acked.retain(|k, _| k.0 != group);
                offsets.maybe.retain(|k, _| k.0 != group);
                offsets.maybe_gone.retain(|k| k.0 != group);
            }
            Err(_) => {
                let places: Vec<Place> = offsets
                    .acked
                    .keys()
                    .chain(offsets.maybe.keys())
                    .filter(|k| k.0 == group)
                    .cloned()
                    .collect();
                offsets.maybe_gone.extend(places);
                self.store = None;
            }
        }
        "delete_group_offsets"
    }
}
