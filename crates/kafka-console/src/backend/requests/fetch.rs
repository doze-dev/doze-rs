//! Fetch: one partition, from one offset, never waiting, never more than the limit.

use kafka_wire::StrBytes;
use kafka_wire::Uuid;
use kafka_wire::api::Request;
use kafka_wire::messages::FetchRequest;
use kafka_wire::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_wire::messages::{BrokerId, TopicName};

/// What a table of records reads from one partition at a time: 256 KiB.
pub const TABLE_BYTES: i32 = 256 << 10;
/// What one record is read with: Kafka's default `max.message.bytes` and a batch header over it (1 MiB and 12 bytes).
/// A Fetch returns the first batch whole even if it is larger, so a bigger record still arrives.
pub const RECORD_BYTES: i32 = (1 << 20) + 12;

/// The isolation level of a read: what a consumer of that level is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Isolation {
    /// Everything up to the high watermark (0).
    ReadUncommitted,
    /// Up to the last stable offset, skipping what was aborted (1).
    ReadCommitted,
}

impl Isolation {
    /// The protocol's number.
    #[must_use]
    pub const fn level(self) -> i8 {
        match self {
            Self::ReadUncommitted => 0,
            Self::ReadCommitted => 1,
        }
    }
}

/// A Fetch of one partition of topic `topic_id` from `offset`, returning at most `max_bytes` (the first batch is
/// whole whatever its size), at once: `max_wait 0`, so an empty partition answers empty.
#[must_use]
pub fn fetch_partition(
    topic: &str,
    topic_id: Uuid,
    partition: i32,
    offset: i64,
    max_bytes: i32,
    isolation: Isolation,
) -> Request {
    let mut p = FetchPartition::default();
    p.partition = partition;
    p.fetch_offset = offset;
    p.partition_max_bytes = max_bytes;
    p.current_leader_epoch = -1;
    p.last_fetched_epoch = -1;
    p.log_start_offset = -1;
    let mut t = FetchTopic::default();
    // A version before 13 names the topic, from 13 on it is the id: both are given, and the broker reads the one its version has.
    t.topic = TopicName(StrBytes::from_string(topic.to_owned()));
    t.topic_id = topic_id;
    t.partitions = vec![p];
    let mut r = FetchRequest::default();
    r.replica_id = BrokerId(-1);
    r.max_wait_ms = 0;
    r.min_bytes = 0;
    r.max_bytes = max_bytes;
    r.isolation_level = isolation.level();
    r.topics = vec![t];
    Request::Fetch(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fetch_never_waits_and_is_bounded_by_the_limit_it_was_given() {
        let Request::Fetch(r) = fetch_partition(
            "orders",
            Uuid::from_u128(7),
            2,
            40,
            1000,
            Isolation::ReadCommitted,
        ) else {
            panic!("not a Fetch");
        };
        assert_eq!((r.max_wait_ms, r.min_bytes, r.max_bytes), (0, 0, 1000));
        assert_eq!(r.isolation_level, 1);
        assert_eq!(r.replica_id.0, -1);
        let p = &r.topics[0].partitions[0];
        assert_eq!(
            (p.partition, p.fetch_offset, p.partition_max_bytes),
            (2, 40, 1000)
        );
        assert_eq!(r.topics[0].topic_id, Uuid::from_u128(7));
        assert_eq!(
            r.topics[0].topic.0.as_str(),
            "orders",
            "a version before 13 reads the name"
        );
    }

    #[test]
    fn the_isolation_levels_are_the_protocols_numbers() {
        assert_eq!(Isolation::ReadUncommitted.level(), 0);
        assert_eq!(Isolation::ReadCommitted.level(), 1);
        assert_eq!(RECORD_BYTES, 1_048_588);
    }
}
