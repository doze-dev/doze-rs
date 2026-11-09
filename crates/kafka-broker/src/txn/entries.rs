//! The transactional ids there are: an entry for each, behind a lock of its own.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use kafka_meta::TxnRecord;
use tokio::sync::{OwnedRwLockWriteGuard, RwLock};

/// A transaction held for reading, shared by the partitions of one Produce request.
pub(crate) type Held = Arc<tokio::sync::OwnedRwLockReadGuard<Entry>>;

/// One transactional id.
#[derive(Debug)]
pub(crate) struct Entry {
    pub(crate) rec: TxnRecord,
    /// The last reason the transaction's decision could not be carried out, so that it is logged once and not on every
    /// attempt.
    pub(crate) failure: Option<String>,
    /// Forgotten (the id expired): whoever still holds this entry must look the id up again.
    pub(crate) retired: bool,
}

/// The entries. A transaction's lock is held for reading across a transactional batch's check and append (so that a
/// transaction cannot be ended between the two and leave a record behind its own marker), and for writing by everything
/// that changes it.
#[derive(Debug, Default)]
pub(crate) struct Txns {
    entries: Mutex<HashMap<String, Arc<RwLock<Entry>>>>,
}

impl Txns {
    pub(crate) fn load(records: Vec<TxnRecord>) -> Self {
        let entries = records
            .into_iter()
            .map(|rec| {
                (
                    rec.id.clone(),
                    Arc::new(RwLock::new(Entry {
                        rec,
                        failure: None,
                        retired: false,
                    })),
                )
            })
            .collect();
        Self {
            entries: Mutex::new(entries),
        }
    }

    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<RwLock<Entry>>>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn get(&self, id: &str) -> Option<Arc<RwLock<Entry>>> {
        self.map().get(id).cloned()
    }

    pub(crate) fn get_or_create(&self, id: &str) -> Arc<RwLock<Entry>> {
        Arc::clone(self.map().entry(id.to_owned()).or_insert_with(|| {
            Arc::new(RwLock::new(Entry {
                rec: TxnRecord::new(id),
                failure: None,
                retired: false,
            }))
        }))
    }

    /// The id's entry locked for writing, or `None` if there is none.
    pub(crate) async fn lock(&self, id: &str) -> Option<OwnedRwLockWriteGuard<Entry>> {
        loop {
            let guard = self.get(id)?.write_owned().await;
            if !guard.retired {
                return Some(guard);
            }
        }
    }

    /// The id's entry locked for writing, made if there is none.
    pub(crate) async fn lock_or_create(&self, id: &str) -> OwnedRwLockWriteGuard<Entry> {
        loop {
            let guard = self.get_or_create(id).write_owned().await;
            if !guard.retired {
                return guard;
            }
        }
    }

    /// Forgets an entry (while its lock is held by the caller, who has retired it).
    pub(crate) fn remove(&self, id: &str, entry: &Arc<RwLock<Entry>>) {
        let mut map = self.map();
        if map.get(id).is_some_and(|e| Arc::ptr_eq(e, entry)) {
            map.remove(id);
        }
    }

    pub(crate) fn all(&self) -> Vec<Arc<RwLock<Entry>>> {
        self.map().values().cloned().collect()
    }
}
