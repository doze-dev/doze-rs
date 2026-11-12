//! Broker metadata: topics, the producer ids handed out, and the offsets groups have committed. (Transactions and
//! ACLs join them as the phases that need them arrive.)
//!
//! One redb database on the file seam of `kafka-storage`, so the simulated disk that crash-tests the logs
//! crash-tests this too. The records are ours (a version byte and postcard), not redb's types.

mod acl;
mod backend;
mod error;
mod life;
mod names;
mod offsets;
mod record;
mod store;
mod txn;

pub use acl::AclRecord;
pub use error::MetaError;
pub use life::{ConsumerLife, GroupLife};
pub use names::Collision;
pub use offsets::{Committed, GroupOffset, OffsetCommit, StoredOffset};
pub use record::Topic;
pub use store::{MetaStore, random_topic_id};
pub use txn::{TxnOffset, TxnRecord, TxnState};
