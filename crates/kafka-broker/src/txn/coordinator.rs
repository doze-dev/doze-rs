//! What the coordinator does with a transactional id, under its lock: take it over, end its transaction (markers on
//! disk, then the offsets), finish what a stop left half done, and abort what outlives its timeout.

use kafka_meta::{TxnRecord, TxnState};
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;

use super::entries::{Entry, Held};
use super::rules::{self, End};
use crate::{Broker, blocking, clock};

impl<F: Fs> Broker<F> {
    /// Changes an entry: `change` is applied to a copy of its record, the copy is stored, and only then does the entry
    /// take it. What is in memory never says more than the store does: a write that fails changes nothing, and the
    /// request is answered NOT_COORDINATOR (what Kafka answers when its transaction log cannot be appended to).
    pub(crate) async fn update_txn(
        &self,
        entry: &mut Entry,
        change: impl FnOnce(&mut TxnRecord),
    ) -> Result<(), E> {
        let mut next = entry.rec.clone();
        change(&mut next);
        next.updated_ms = clock::now().get();
        match blocking::run(|| self.meta.put_txn(&next)) {
            Ok(()) => {
                entry.rec = next;
                Ok(())
            }
            Err(e) => {
                tracing::error!(transactional_id = %entry.rec.id, error = %e, "transactions: could not store the state of a transaction");
                Err(E::NotCoordinator)
            }
        }
    }

    /// InitProducerId for a transactional id: makes the caller the owner, fences whoever held the id, and decides what
    /// was left open. The producer id and epoch it is to use.
    pub(crate) async fn init_txn_producer(
        &self,
        id: &str,
        timeout_ms: i32,
    ) -> Result<(i64, i16), E> {
        if timeout_ms <= 0 || timeout_ms > rules::MOST_TIMEOUT_MS {
            return Err(E::InvalidTransactionTimeout);
        }
        let entry = self.txns.get_or_create(id);
        let mut e = entry.write().await;
        let (mut producer, mut epoch) = (e.rec.producer_id, e.rec.epoch);
        match e.rec.state {
            TxnState::Empty if producer < 0 => {
                producer = self.next_txn_producer_id()?;
                epoch = -1;
            }
            TxnState::Ongoing | TxnState::PrepareAbort => {
                // The previous owner's transaction is aborted; its markers carry the new epoch.
                self.abort_fencing(&mut e).await?;
                (producer, epoch) = (e.rec.producer_id, e.rec.epoch);
            }
            TxnState::PrepareCommit => self.finish_txn(&mut e, true).await?,
            _ => {}
        }
        if epoch >= i16::MAX - 1 {
            // Epochs are used up: a new producer id, and epoch 0 again.
            producer = self.next_txn_producer_id()?;
            epoch = -1;
        }
        self.update_txn(&mut e, |rec| {
            rec.producer_id = producer;
            rec.epoch = epoch + 1;
            rec.state = TxnState::Empty;
            rec.timeout_ms = timeout_ms;
            rec.partitions.clear();
            rec.groups.clear();
            rec.offsets.clear();
        })
        .await?;
        Ok((e.rec.producer_id, e.rec.epoch))
    }

    fn next_txn_producer_id(&self) -> Result<i64, E> {
        blocking::run(|| self.meta.next_producer_id()).map_err(|e| {
            tracing::error!(error = %e, "transactions: could not allocate a producer id");
            E::NotCoordinator
        })
    }

    /// Aborts an open transaction and fences its producer by raising the epoch first, so the markers (and whatever the
    /// old producer tries next) carry the new one.
    pub(crate) async fn abort_fencing(&self, e: &mut Entry) -> Result<(), E> {
        self.update_txn(e, |rec| {
            rec.epoch = rec.epoch.saturating_add(1);
            rec.state = TxnState::PrepareAbort;
        })
        .await?;
        self.finish_txn(e, false).await
    }

    /// EndTxn: the producer's decision to commit or abort. A decision, once stored, is never changed and never answered
    /// as its opposite.
    pub(crate) async fn end_txn(
        &self,
        id: &str,
        producer: i64,
        epoch: i16,
        commit: bool,
    ) -> Option<E> {
        let Some(entry) = self.txns.get(id) else {
            return Some(E::InvalidProducerIdMapping);
        };
        let mut e = entry.write().await;
        if let Some(refused) = rules::owns(&e.rec, producer, epoch) {
            return Some(refused);
        }
        match rules::end(&e.rec, commit) {
            End::Answer(answer) => answer,
            End::Decide { commit } => {
                let state = if commit {
                    TxnState::PrepareCommit
                } else {
                    TxnState::PrepareAbort
                };
                if let Err(refused) = self.update_txn(&mut e, |rec| rec.state = state).await {
                    return Some(refused);
                }
                self.finish_txn(&mut e, commit).await.err()
            }
            End::Finish { decided, asked } => {
                if let Err(refused) = self.finish_txn(&mut e, decided).await {
                    return Some(refused);
                }
                // The other decision was made, and has been carried out.
                (asked != decided).then_some(E::InvalidTxnState)
            }
        }
    }

    /// Finishes what a stop left half done: a transaction whose decision was stored is carried out. One still open is
    /// left open, as in Kafka: its producer may be alive and retrying across the restart, and aborting it would fence
    /// that producer for good. Its timeout counts again from now, and a producer that is really gone is found by that.
    pub async fn recover_transactions(&self) {
        for entry in self.txns.all() {
            let mut e = entry.write().await;
            let outcome = match e.rec.state {
                TxnState::PrepareCommit => self.finish_txn(&mut e, true).await,
                TxnState::PrepareAbort => self.finish_txn(&mut e, false).await,
                TxnState::Ongoing => self.update_txn(&mut e, |_| {}).await,
                _ => Ok(()),
            };
            if let Err(code) = outcome {
                tracing::error!(transactional_id = %e.rec.id, ?code, "transactions: could not finish a transaction left from before the restart");
            }
        }
    }

    /// One look at the transactions: a decision that could not be carried out is carried out when it can be, and an open
    /// transaction past its timeout is aborted, which raises the epoch so that a producer that was only slow finds
    /// itself fenced instead of writing into a transaction that is already decided.
    pub async fn check_transactions(&self) {
        for entry in self.txns.all() {
            let (decided, expired) = {
                let e = entry.read().await;
                (
                    matches!(
                        e.rec.state,
                        TxnState::PrepareCommit | TxnState::PrepareAbort
                    ),
                    rules::expired(&e.rec, clock::now().get()),
                )
            };
            if !decided && !expired {
                continue;
            }
            let mut e = entry.write().await;
            match e.rec.state {
                TxnState::PrepareCommit => {
                    let _ = self.finish_txn(&mut e, true).await;
                }
                TxnState::PrepareAbort => {
                    let _ = self.finish_txn(&mut e, false).await;
                }
                TxnState::Ongoing if rules::expired(&e.rec, clock::now().get()) => {
                    tracing::info!(transactional_id = %e.rec.id, timeout_ms = e.rec.timeout_ms, "transactions: aborting a transaction that timed out");
                    if let Err(code) = self.abort_fencing(&mut e).await {
                        tracing::error!(transactional_id = %e.rec.id, ?code, "transactions: could not abort a timed-out transaction");
                    }
                }
                _ => {}
            }
        }
    }
}

impl<F: Fs> Broker<F> {
    /// Takes the transaction of `id` for reading, to be held across the appends of one Produce request: it keeps the
    /// transaction from being ended between a batch's check and its append. Taken once for the whole request, and not for
    /// each partition: a lock that is waited for again while an earlier hold of it is kept would wait behind a writer
    /// that is itself waiting for the first, and neither would ever go on (the lock queues in order).
    pub(crate) async fn hold_txn(&self, id: Option<&str>) -> Option<Held> {
        let entry = self.txns.get(id.filter(|id| !id.is_empty())?)?;
        Some(std::sync::Arc::new(entry.read_owned().await))
    }

    /// The check a transactional batch has to pass: the request must name the transactional id, the producer must own it,
    /// a transaction must be open, and the partition must have been added to it. `held` is [`Self::hold_txn`] of the id.
    pub(crate) fn check_txn_batch(
        id: Option<&str>,
        held: Option<&Held>,
        who: (i64, i16),
        at: (&str, i32),
    ) -> Result<(), (E, &'static str)> {
        if id.is_none_or(str::is_empty) {
            return Err((
                E::InvalidTxnState,
                "a transactional batch must come in a produce request that names the transactional id",
            ));
        }
        let Some(held) = held else {
            return Err((
                E::InvalidProducerIdMapping,
                "no producer owns that transactional id",
            ));
        };
        rules::may_write(&held.rec, who.0, who.1, at.0, at.1).map_err(|code| {
            let why = match code {
                E::InvalidTxnState => "the partition was not added to an open transaction",
                _ => "the producer id and epoch are not the ones that own the transactional id",
            };
            (code, why)
        })
    }
}

/// How often open transactions are looked at.
const CHECK_EVERY: std::time::Duration = std::time::Duration::from_millis(500);

impl<F: Fs + Send + Sync + 'static> Broker<F> {
    /// Looks at the transactions now and then as long as the broker lives.
    pub fn start_transactions(self: &std::sync::Arc<Self>) -> tokio::task::JoinHandle<()> {
        let broker = std::sync::Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(CHECK_EVERY).await;
                let Some(broker) = broker.upgrade() else {
                    break;
                };
                broker.check_transactions().await;
            }
        })
    }
}
