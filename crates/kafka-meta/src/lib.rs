//! Broker metadata: topics and the producer ids handed out. (Committed offsets, transactions and ACLs join
//! them as the phases that need them arrive.)
//!
//! One redb database on the file seam of `kafka-storage`, so the simulated disk that crash-tests the logs
//! crash-tests this too. The records are ours (a version byte and postcard), not redb's types.

mod backend;
mod error;
mod names;
mod record;
mod store;

pub use error::MetaError;
pub use names::Collision;
pub use record::Topic;
pub use store::{MetaStore, random_topic_id};
