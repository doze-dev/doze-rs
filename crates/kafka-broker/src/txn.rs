//! The transaction coordinator: the classic protocol (before KIP-890), which is what current Java, librdkafka and
//! franz-go clients use when the broker offers Produce no newer than v9.
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

pub(crate) use entries::{Held, Txns};
