//! Forgetting transactional ids nobody has used for a long time (Kafka's `transactional.id.expiration.ms`, seven days).
//! An application that names its transactional id after each run would otherwise leave an entry in memory, and a record
//! in the metadata store, for every run it ever made.

use kafka_storage::fs::Fs;

use super::rules;
use crate::{Broker, blocking, clock};

impl<F: Fs> Broker<F> {
    /// Forgets the ids whose transaction is over and that have not changed for the expiry. An id with an open
    /// transaction is never forgotten: the timeout deals with it first.
    pub(super) async fn expire_txn_ids(&self) {
        let now = clock::now().get();
        let limit =
            i64::try_from(self.config.transactional_id_expiration.as_millis()).unwrap_or(i64::MAX);
        for entry in self.txns.all() {
            if !rules::idle(&entry.read().await.rec, now, limit) {
                continue;
            }
            let mut e = entry.write().await;
            if e.retired || !rules::idle(&e.rec, now, limit) {
                continue;
            }
            // The record goes first: a stop in between leaves an id that is forgotten again at the next look.
            match blocking::run(|| self.meta.delete_txn(&e.rec.id)) {
                Ok(()) => {
                    e.retired = true;
                    self.txns.remove(&e.rec.id, &entry);
                }
                Err(error) => {
                    tracing::error!(transactional_id = %e.rec.id, %error, "transactions: could not forget an expired transactional id");
                }
            }
        }
    }
}
