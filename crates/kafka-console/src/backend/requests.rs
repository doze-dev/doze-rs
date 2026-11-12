//! One builder per request the console sends: typed, bounded, nothing the boards do not draw.

pub mod alter_configs;
mod configs;
mod fetch;
pub mod forms;
mod groups;
mod groups_read;
mod log_reads;
mod metadata;
mod offsets;
mod producers;
pub mod topics_write;

pub use configs::{KEYS as TOPIC_CONFIG_KEYS, describe_topic_configs};
pub use fetch::{Isolation, RECORD_BYTES, TABLE_BYTES, fetch_partition};
pub use groups::{committed_of, list_groups};
pub use groups_read::{
    MOST_DESCRIBED, admin_commit, consumer_group_describe, delete_group, describe_groups,
    offset_delete, offsets_of,
};
pub use log_reads::list_all_transactions;
pub use metadata::metadata_topics;
pub use offsets::{Ask, EARLIEST, LATEST, list_offsets};
pub use producers::describe_producers;

use kafka_wire::StrBytes;
use kafka_wire::api::Request;
use kafka_wire::messages::list_offsets_request::{ListOffsetsPartition, ListOffsetsTopic};
use kafka_wire::messages::{
    DescribeTransactionsRequest, ListOffsetsRequest, ListTransactionsRequest, MetadataRequest,
    TopicName, TransactionalId,
};

/// How many transactional ids one DescribeTransactions asks about.
pub const DESCRIBE_CHUNK: usize = 100;

/// Metadata for every topic, creating none (the console never makes a topic by asking about it).
#[must_use]
pub fn metadata_all() -> Request {
    let mut r = MetadataRequest::default();
    r.topics = None;
    r.allow_auto_topic_creation = false;
    Request::Metadata(r)
}

/// Metadata for no topic: the brokers alone, which is all the advertised-listener check wants.
#[must_use]
pub fn metadata_brokers() -> Request {
    let mut r = MetadataRequest::default();
    r.topics = Some(Vec::new());
    r.allow_auto_topic_creation = false;
    Request::Metadata(r)
}

/// The transactions that are open (state `Ongoing`).
#[must_use]
pub fn list_ongoing_transactions() -> Request {
    let mut r = ListTransactionsRequest::default();
    r.state_filters = vec![StrBytes::from_static_str("Ongoing")];
    Request::ListTransactions(r)
}

/// The detail of these transactional ids. The broker takes any number; the callers ask in chunks of
/// [`DESCRIBE_CHUNK`] so that no one call is long.
#[must_use]
pub fn describe_transactions(ids: &[String]) -> Request {
    let mut r = DescribeTransactionsRequest::default();
    r.transactional_ids = ids
        .iter()
        .map(|id| TransactionalId(StrBytes::from_string(id.clone())))
        .collect();
    Request::DescribeTransactions(r)
}

/// The latest offset of one partition, as a consumer of that isolation level sees it (`read_committed` is 1: the last
/// stable offset; `read_uncommitted` is 0: the high watermark).
#[must_use]
pub fn latest_offset(topic: &str, partition: i32, isolation_level: i8) -> Request {
    let mut p = ListOffsetsPartition::default();
    p.partition_index = partition;
    p.current_leader_epoch = -1;
    // -1 is "the latest offset" in ListOffsets.
    p.timestamp = -1;
    let mut t = ListOffsetsTopic::default();
    t.name = TopicName(StrBytes::from_string(topic.to_owned()));
    t.partitions = vec![p];
    let mut r = ListOffsetsRequest::default();
    r.replica_id = kafka_wire::messages::BrokerId(-1);
    r.isolation_level = isolation_level;
    r.topics = vec![t];
    Request::ListOffsets(r)
}

pub mod acls;
pub mod broker_configs;
