//! ListOffsets for the partitions of a topic: earliest, latest and by time, for either isolation level.

use kafka_wire::StrBytes;
use kafka_wire::api::Request;
use kafka_wire::messages::list_offsets_request::{ListOffsetsPartition, ListOffsetsTopic};
use kafka_wire::messages::{BrokerId, ListOffsetsRequest, TopicName};

use super::fetch::Isolation;

/// The timestamp that asks for the earliest offset (the log start).
pub const EARLIEST: i64 = -2;
/// The timestamp that asks for the latest offset (the log end, or the last stable offset for `read_committed`).
pub const LATEST: i64 = -1;

/// One ask of a ListOffsets: a partition and what is asked of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ask {
    pub partition: i32,
    /// [`EARLIEST`], [`LATEST`] or a time in milliseconds (the first offset at or after it).
    pub timestamp: i64,
}

/// A ListOffsets of one topic. A partition may be asked twice (its earliest and its latest in one request: the
/// broker answers each entry). At most 4,096 entries.
#[must_use]
pub fn list_offsets(topic: &str, asks: &[Ask], isolation: Isolation) -> Request {
    let mut t = ListOffsetsTopic::default();
    t.name = TopicName(StrBytes::from_string(topic.to_owned()));
    t.partitions = asks
        .iter()
        .take(4096)
        .map(|a| {
            let mut p = ListOffsetsPartition::default();
            p.partition_index = a.partition;
            p.current_leader_epoch = -1;
            p.timestamp = a.timestamp;
            p
        })
        .collect();
    let mut r = ListOffsetsRequest::default();
    r.replica_id = BrokerId(-1);
    r.isolation_level = isolation.level();
    r.topics = vec![t];
    Request::ListOffsets(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_ask_is_one_entry_in_order_and_the_isolation_is_the_requests() {
        let asks = [
            Ask {
                partition: 0,
                timestamp: EARLIEST,
            },
            Ask {
                partition: 0,
                timestamp: LATEST,
            },
            Ask {
                partition: 1,
                timestamp: 1_700_000_000_000,
            },
        ];
        let Request::ListOffsets(r) = list_offsets("orders", &asks, Isolation::ReadCommitted)
        else {
            panic!("not ListOffsets");
        };
        assert_eq!(r.isolation_level, 1);
        let got: Vec<(i32, i64)> = r.topics[0]
            .partitions
            .iter()
            .map(|p| (p.partition_index, p.timestamp))
            .collect();
        assert_eq!(got, vec![(0, -2), (0, -1), (1, 1_700_000_000_000)]);
    }

    #[test]
    fn a_request_is_bounded() {
        let asks: Vec<Ask> = (0..10_000)
            .map(|i| Ask {
                partition: i,
                timestamp: LATEST,
            })
            .collect();
        let Request::ListOffsets(r) = list_offsets("t", &asks, Isolation::ReadUncommitted) else {
            panic!("not ListOffsets");
        };
        assert_eq!(r.topics[0].partitions.len(), 4096);
    }
}
