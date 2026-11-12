//! ACLs in the workload: added and removed, and checked after a failure or a power cut.

use std::collections::HashSet;

use kafka_meta::{AclRecord, MetaStore};

use super::{Model, World};

const NAMES: [&str; 4] = ["orders", "ord", "*", "payments"];
const WHO: [&str; 3] = ["User:alice", "User:bob", "User:*"];

/// The ACLs the workload has been told of.
#[derive(Default)]
pub(crate) struct Acls {
    /// Added and acknowledged, and not acknowledged as removed since.
    pub(crate) acked: HashSet<AclRecord>,
    /// Added by a write that failed part-way: they may or may not be there.
    pub(crate) maybe_added: HashSet<AclRecord>,
    /// In `acked`, and removed by a write that failed part-way: they may or may not still be there.
    pub(crate) maybe_removed: HashSet<AclRecord>,
}

impl Model {
    pub(super) fn check_acls(&self, store: &MetaStore) -> Result<(), String> {
        let found: HashSet<AclRecord> = store
            .acls()
            .map_err(|e| e.to_string())?
            .into_iter()
            .collect();
        for acl in &self.acls.acked {
            if !found.contains(acl) && !self.acls.maybe_removed.contains(acl) {
                return Err(format!("{acl:?} was acknowledged and is gone"));
            }
        }
        for acl in &found {
            if !self.acls.acked.contains(acl) && !self.acls.maybe_added.contains(acl) {
                return Err(format!(
                    "{acl:?} is there and was never added, or was removed"
                ));
            }
        }
        Ok(())
    }
}

impl World {
    fn some_acl(&mut self) -> AclRecord {
        let pick = |rng: &mut super::Rng, n: usize| rng.below(n as u64) as usize;
        AclRecord {
            resource_type: 2 + pick(&mut self.rng, 2) as i8,
            name: NAMES[pick(&mut self.rng, NAMES.len())].to_owned(),
            pattern_type: 3 + pick(&mut self.rng, 2) as i8,
            principal: WHO[pick(&mut self.rng, WHO.len())].to_owned(),
            host: "*".to_owned(),
            operation: 3 + pick(&mut self.rng, 6) as i8,
            permission: 2 + pick(&mut self.rng, 2) as i8,
        }
    }

    /// Adds one to three ACLs in one write.
    pub(super) fn add_acls(&mut self) -> &'static str {
        let batch: Vec<AclRecord> = (0..1 + self.rng.below(3))
            .map(|_| self.some_acl())
            .collect();
        let Some(store) = &self.store else {
            return "add_acls";
        };
        if store.add_acls(&batch).is_ok() {
            for acl in batch {
                self.model.acls.maybe_removed.remove(&acl);
                self.model.acls.acked.insert(acl);
            }
        } else {
            self.model.acls.maybe_added.extend(batch);
            self.store = None;
        }
        "add_acls"
    }

    /// Removes one of the ACLs it has been told of (or one that is not there), and a second at random.
    pub(super) fn remove_acls(&mut self) -> &'static str {
        let known: Vec<AclRecord> = self.model.acls.acked.iter().cloned().collect();
        let mut batch = vec![self.some_acl()];
        if !known.is_empty() {
            batch.push(known[self.rng.below(known.len() as u64) as usize].clone());
        }
        let Some(store) = &self.store else {
            return "remove_acls";
        };
        if store.remove_acls(&batch).is_ok() {
            for acl in batch {
                self.model.acls.acked.remove(&acl);
                self.model.acls.maybe_added.remove(&acl);
                self.model.acls.maybe_removed.remove(&acl);
            }
        } else {
            self.model.acls.maybe_removed.extend(
                batch
                    .into_iter()
                    .filter(|a| self.model.acls.acked.contains(a)),
            );
            self.store = None;
        }
        "remove_acls"
    }
}
