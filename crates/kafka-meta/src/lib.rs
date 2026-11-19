//! Broker metadata: topics, the producer ids handed out, and the offsets groups have committed. (Transactions and
//! ACLs join them as the phases that need them arrive.)
//!
//! One redb database on the file seam of `kafka-storage`, so the simulated disk that crash-tests the logs
//! crash-tests this too. The records are ours (a version byte and postcard), not redb's types.

mod acl;
mod credential;
mod error;
mod life;
mod names;
mod offsets;
mod quota;
mod record;
mod settings;
mod share;
mod store;
mod token;
mod txn;

pub use acl::AclRecord;
pub use credential::{CredentialId, CredentialRecord};
pub use error::MetaError;
pub use life::{
    ConsumerLife, GroupLife, ShareLife, StoredSubtopology, StoredTopic, StoredTopology, StreamsLife,
};
pub use names::Collision;
pub use offsets::{Committed, GroupOffset, OffsetCommit, StoredOffset};
pub use quota::{QuotaEntity, QuotaName, QuotaValues};
pub use record::Topic;
pub use settings::{CleanupPolicy, TopicSettings};
pub use share::{ShareBatch, ShareRecordState, ShareState};
pub use store::{FileMode, MetaStore, Records, ShareStateWrite, random_topic_id};
pub use token::TokenRecord;
pub use txn::{TxnOffset, TxnRecord, TxnState, TxnVersion};
