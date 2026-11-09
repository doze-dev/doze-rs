//! Transaction records in the workload: written, and checked after a failure or a power cut.

use std::collections::HashMap;

use kafka_meta::{MetaStore, TxnOffset, TxnRecord, TxnState};

use super::{Model, World};

const IDS: [&str; 4] = ["tx-a", "tx-b", "tx-c", "tx-d"];

/// The records the workload has been told of.
#[derive(Default)]
pub(crate) struct Txns {
    /// Written and acknowledged: the last one.
    pub(crate) acked: HashMap<&'static str, TxnRecord>,
    /// What writes that failed part-way may have left.
    pub(crate) maybe: HashMap<&'static str, Vec<TxnRecord>>,
    /// Ids in `acked` whose record a forget that failed part-way may have removed.
    pub(crate) maybe_gone: std::collections::HashSet<&'static str>,
}

impl Model {
    pub(super) fn check_txns(&self, store: &MetaStore) -> Result<(), String> {
        let found: HashMap<String, TxnRecord> = store
            .txns()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|r| (r.id.clone(), r))
            .collect();
        let allowed = |id: &str, rec: &TxnRecord| {
            self.txns.acked.get(id) == Some(rec)
                || self.txns.maybe.get(id).is_some_and(|m| m.contains(rec))
        };
        for (id, want) in &self.txns.acked {
            match found.get(*id) {
                Some(rec) if allowed(id, rec) => {}
                Some(rec) => {
                    return Err(format!(
                        "transaction {id} came back as {rec:?}, was {want:?}"
                    ));
                }
                None if self.txns.maybe_gone.contains(id) => {}
                None => return Err(format!("transaction {id} was acknowledged and is gone")),
            }
        }
        for (id, rec) in &found {
            if !IDS.contains(&id.as_str()) || !allowed(id, rec) {
                return Err(format!("{id} holds {rec:?}, which was never written"));
            }
        }
        Ok(())
    }
}

impl World {
    pub(super) fn txn(&mut self) -> &'static str {
        let id = IDS[self.rng.below(IDS.len() as u64) as usize];
        let mut rec = TxnRecord::new(id);
        rec.producer_id = 1_000 + self.rng.below(50) as i64;
        rec.epoch = self.rng.below(5) as i16;
        rec.timeout_ms = 60_000;
        rec.state = [
            TxnState::Empty,
            TxnState::Ongoing,
            TxnState::PrepareCommit,
            TxnState::PrepareAbort,
            TxnState::CompleteCommit,
            TxnState::CompleteAbort,
        ][self.rng.below(6) as usize];
        rec.updated_ms = self.rng.below(1_000_000) as i64;
        rec.partitions = (0..self.rng.below(3))
            .map(|i| ("t".to_owned(), i as i32))
            .collect();
        if self.rng.below(2) == 0 {
            rec.groups = vec!["g".to_owned()];
            rec.offsets = vec![TxnOffset {
                group: "g".into(),
                topic: "t".into(),
                partition: 0,
                offset: self.rng.below(100) as i64,
                leader_epoch: -1,
                metadata: String::new(),
            }];
        }
        let Some(store) = &self.store else {
            return "txn";
        };
        if store.put_txn(&rec).is_ok() {
            self.model.txns.maybe_gone.remove(id);
            self.model.txns.acked.insert(id, rec);
        } else {
            self.model.txns.maybe.entry(id).or_default().push(rec);
            self.store = None;
        }
        "txn"
    }

    /// Forgets a transactional id.
    pub(super) fn forget_txn(&mut self) -> &'static str {
        let id = IDS[self.rng.below(IDS.len() as u64) as usize];
        let Some(store) = &self.store else {
            return "forget_txn";
        };
        if store.delete_txn(id).is_ok() {
            self.model.txns.acked.remove(id);
            self.model.txns.maybe.remove(id);
            self.model.txns.maybe_gone.remove(id);
        } else {
            self.model.txns.maybe_gone.insert(id);
            self.store = None;
        }
        "forget_txn"
    }
}
