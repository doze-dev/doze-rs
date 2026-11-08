//! Group records in the workload: written, and checked after a failure or a power cut.

use std::collections::{HashMap, HashSet};

use kafka_meta::{GroupLife, MetaStore};
use kafka_wire::ids::GroupId;

use super::{GROUPS, Model, World};

/// The records the workload has been told of.
#[derive(Default)]
pub(crate) struct Lives {
    /// Written and acknowledged: the last one.
    pub(crate) acked: HashMap<&'static str, GroupLife>,
    /// What writes that failed part-way may have left.
    pub(crate) maybe: HashMap<&'static str, Vec<GroupLife>>,
}

impl Model {
    pub(super) fn check_lives(&self, store: &MetaStore) -> Result<(), String> {
        let found: HashMap<String, GroupLife> = store
            .group_lives()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|(g, l)| (g.to_string(), l))
            .collect();
        let allowed = |group: &str, life: &GroupLife| {
            self.lives.acked.get(group) == Some(life)
                || self
                    .lives
                    .maybe
                    .get(group)
                    .is_some_and(|m| m.contains(life))
        };
        for (group, want) in &self.lives.acked {
            match found.get(*group) {
                Some(life) if allowed(group, life) => {}
                Some(life) => {
                    return Err(format!(
                        "record of {group} came back as {life:?}, was {want:?}"
                    ));
                }
                None => {
                    return Err(format!(
                        "the record of {group} was acknowledged and is gone"
                    ));
                }
            }
        }
        let known: HashSet<&str> = GROUPS.into_iter().collect();
        for (group, life) in &found {
            if !known.contains(group.as_str()) || !allowed(group, life) {
                return Err(format!("{group} holds {life:?}, which was never written"));
            }
        }
        Ok(())
    }
}

impl World {
    pub(super) fn life(&mut self) -> &'static str {
        let group = GROUPS[self.rng.below(GROUPS.len() as u64) as usize];
        let live = self.rng.below(2) == 0;
        let life = GroupLife {
            live,
            session_ms: 6_000 + self.rng.below(60_000) as u32,
            empty_since_ms: (!live).then(|| self.rng.below(1_000_000) as i64),
            protocol_type: "consumer".into(),
        };
        let Some(store) = &self.store else {
            return "life";
        };
        if store
            .set_group_life(&GroupId::parse(group).unwrap(), &life)
            .is_ok()
        {
            self.model.lives.acked.insert(group, life);
        } else {
            self.model.lives.maybe.entry(group).or_default().push(life);
            self.store = None;
        }
        "life"
    }
}
