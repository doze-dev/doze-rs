//! The transaction coordinator: the classic protocol, and KIP-890's two parts (the broker refuses a transactional write
//! to a partition the transaction does not have, with the abortable error from the versions that know it; and from
//! Produce v12, EndTxn v5 and TxnOffsetCommit v5, a write adds its partition and every end raises the epoch). Which a
//! request speaks is its version; a client may change from one to the other between two transactions.
//!
//! One record per `transactional.id` holds the producer id and epoch that own it and where its transaction stands. The
//! record is written *before* the broker acts on a change, so a stop in the middle of ending a transaction is finished
//! on the next start. The rules (who owns the id, what may be added when, what each EndTxn is answered) are pure
//! ([`rules`]); the coordinator ([`coordinator`]) does the writing under a lock per transactional id.

mod coordinator;
mod entries;
mod expiry;
mod finish;
pub(crate) mod rules;

pub(crate) use crate::alarm::Alarm;
#[cfg(feature = "inspect")]
pub(crate) use coordinator::CHECK_EVERY;
pub(crate) use coordinator::why;
pub(crate) use entries::{Held, Txns};
