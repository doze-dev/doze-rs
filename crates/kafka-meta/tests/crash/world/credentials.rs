//! SCRAM credentials in the workload: put, removed and altered together, and checked after a failure or a power cut.

use std::collections::{HashMap, HashSet};

use kafka_meta::{CredentialId, CredentialRecord, MetaStore};

use super::{Model, World};

const USERS: [&str; 4] = ["alice", "bob", "carol", "a\0b"];

/// The credentials the workload has been told of.
#[derive(Default)]
pub(crate) struct Credentials {
    /// Stored and acknowledged, and not acknowledged as removed since.
    pub(crate) acked: HashMap<CredentialId, CredentialRecord>,
    /// Ids that a write which failed part-way may have stored (with any of the records it carried), or replaced.
    pub(crate) maybe_put: HashMap<CredentialId, Vec<CredentialRecord>>,
    /// Ids in `acked` that a write which failed part-way may have removed.
    pub(crate) maybe_removed: HashSet<CredentialId>,
}

impl Model {
    pub(super) fn check_credentials(&self, store: &MetaStore) -> Result<(), String> {
        let found: HashMap<CredentialId, CredentialRecord> = store
            .credentials()
            .map_err(|e| e.to_string())?
            .into_iter()
            .collect();
        let c = &self.credentials;
        for (id, record) in &c.acked {
            match found.get(id) {
                Some(f) if f == record => {}
                Some(f) if c.maybe_put.get(id).is_some_and(|rs| rs.contains(f)) => {}
                None if c.maybe_removed.contains(id) => {}
                Some(_) => return Err(format!("{id:?} came back as another record")),
                None => return Err(format!("{id:?} was acknowledged and is gone")),
            }
        }
        for (id, record) in &found {
            let acked = c.acked.get(id) == Some(record);
            let maybe = c.maybe_put.get(id).is_some_and(|rs| rs.contains(record));
            if !acked && !maybe {
                return Err(format!(
                    "{id:?} is there and was never stored, or was removed, or has a record never written"
                ));
            }
        }
        Ok(())
    }
}

impl World {
    fn some_id(&mut self) -> CredentialId {
        CredentialId {
            user: USERS[self.rng.below(USERS.len() as u64) as usize].to_owned(),
            mechanism: 1 + self.rng.below(2) as i8,
        }
    }

    fn some_record(&mut self) -> CredentialRecord {
        let tag = self.rng.below(250) as u8;
        CredentialRecord {
            salt: vec![tag; 1 + self.rng.below(20) as usize],
            iterations: 4096 + self.rng.below(100) as u32,
            stored_key: vec![tag; 32],
            server_key: vec![tag.wrapping_add(1); 32],
        }
    }

    /// Stores one to three credentials, and sometimes removes another in the same write.
    pub(super) fn alter_credentials(&mut self) -> &'static str {
        let mut puts: Vec<(CredentialId, CredentialRecord)> = Vec::new();
        for _ in 0..1 + self.rng.below(3) {
            let (id, record) = (self.some_id(), self.some_record());
            puts.retain(|(i, _)| *i != id);
            puts.push((id, record));
        }
        let known: Vec<CredentialId> = self.model.credentials.acked.keys().cloned().collect();
        let mut removals = Vec::new();
        if self.rng.below(2) == 0 {
            removals.push(self.some_id());
            if !known.is_empty() {
                removals.push(known[self.rng.below(known.len() as u64) as usize].clone());
            }
        }
        // A removal of what the same write stores is applied first, so the put wins.
        let Some(store) = &self.store else {
            return "alter_credentials";
        };
        let c = &mut self.model.credentials;
        if store.alter_credentials(&puts, &removals).is_ok() {
            for id in &removals {
                c.acked.remove(id);
                c.maybe_put.remove(id);
                c.maybe_removed.remove(id);
            }
            for (id, record) in puts {
                c.maybe_put.remove(&id);
                c.maybe_removed.remove(&id);
                c.acked.insert(id, record);
            }
        } else {
            for id in removals {
                if c.acked.contains_key(&id) {
                    c.maybe_removed.insert(id);
                }
            }
            for (id, record) in puts {
                c.maybe_put.entry(id).or_default().push(record);
            }
            self.store = None;
        }
        "alter_credentials"
    }
}
