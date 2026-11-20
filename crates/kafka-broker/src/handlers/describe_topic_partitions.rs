//! DescribeTopicPartitions (API 75, KIP-966): the partitions of some topics, or of all, a page at a time. What
//! `kafka-topics.sh --describe` and the Java admin client's `describeTopics` ask from Kafka 3.7 on.
//!
//! Kafka 4.4.0's `DescribeTopicPartitionsRequestHandler` and `KRaftMetadataCache.describeTopicResponse`, step by step:
//!
//! - No topic named asks for every topic; each topic named is asked once. Only names at or after the cursor's topic count
//!   (Java's `String.compareTo`), and a cursor whose topic is not among those named, or whose partition is negative, is
//!   INVALID_REQUEST for every topic named, in the request's order.
//! - A topic the caller may not DESCRIBE is not looked for: asked by name it comes last, TOPIC_AUTHORIZATION_FAILED with a
//!   zero id; in a request for all of them it is left out.
//! - The rest go in name order. A page holds at most `min(response_partition_limit, 2000)` partitions (and at least one); a
//!   topic cut short ends the page with a cursor at its next partition, and a topic reached with the page full ends it with
//!   a cursor at its partition 0. A missing topic named is UNKNOWN_TOPIC_OR_PARTITION, or INVALID_TOPIC_EXCEPTION for a name
//!   no topic could have, and takes no room.
//! - Every partition is led by this broker at epoch 0, with this broker its one replica and in-sync replica, no replica
//!   offline, and no eligible leader replicas (an empty list, as Kafka's registration holds when ELR is off or empty).
//! - Each entry but the refused ones carries the caller's operations on the topic.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::ids::TopicName;
use kafka_wire::messages::describe_topic_partitions_response::{
    Cursor, DescribeTopicPartitionsResponsePartition, DescribeTopicPartitionsResponseTopic,
};
use kafka_wire::messages::{
    BrokerId, DescribeTopicPartitionsRequest, DescribeTopicPartitionsResponse,
    TopicName as WireName,
};

use super::streams_group_heartbeat::valid_topic_name;
use crate::{Broker, Caller};

/// Kafka's `max.request.partition.size.limit` default: the most partitions one answer holds.
pub(crate) const MAX_PARTITIONS: i32 = 2000;

/// The topics Kafka keeps for itself (`Topic.isInternal`).
const INTERNAL: [&str; 3] = [
    "__consumer_offsets",
    "__transaction_state",
    "__share_group_state",
];

impl<F: Fs> Broker<F> {
    /// One page of the topics asked for (all of them if none is named), with their partitions.
    pub(crate) fn describe_topic_partitions(
        &self,
        caller: &Caller,
        req: &DescribeTopicPartitionsRequest,
    ) -> DescribeTopicPartitionsResponse {
        let all = req.topics.is_empty();
        let cursor_topic = req.cursor.as_ref().map_or("", |c| c.topic_name.0.as_str());
        let after_cursor = |name: &str| java_cmp(name, cursor_topic) != Ordering::Less;
        let names: BTreeSet<JavaOrdered> = if all {
            (self.meta.topics().iter())
                .map(|t| t.name.to_string())
                .filter(|n| after_cursor(n))
                .map(JavaOrdered)
                .collect()
        } else {
            (req.topics.iter())
                .map(|t| t.name.0.to_string())
                .filter(|n| after_cursor(n))
                .map(JavaOrdered)
                .collect()
        };
        if let Some(cursor) = &req.cursor {
            let named = all || names.iter().any(|n| n.0 == cursor_topic);
            if !named || cursor.partition_index < 0 {
                return invalid_request(req);
            }
        }
        let mut refused = Vec::new();
        let mut allowed = Vec::new();
        for name in names {
            if self.topic_allows(caller, &name.0, Operation::Describe) {
                allowed.push(name.0);
            } else if !all {
                refused.push(entry(
                    &name.0,
                    ResponseError::TopicAuthorizationFailed.code(),
                ));
            }
        }
        let start_of = |name: &str| match &req.cursor {
            Some(c) if name == cursor_topic => c.partition_index,
            _ => 0,
        };
        let limit = req.response_partition_limit.clamp(1, MAX_PARTITIONS);
        let mut resp = self.page(&allowed, start_of, limit, all);
        for t in &mut resp.topics {
            if let Some(name) = &t.name {
                t.topic_authorized_operations = self.topic_operations(caller, &name.0);
            }
        }
        resp.topics.extend(refused);
        resp
    }

    /// `KRaftMetadataCache.describeTopicResponse`: the topics in order until `limit` partitions are in, the cursor after the
    /// last. A missing topic is an error entry unless every topic was asked for (then it was deleted meanwhile).
    fn page(
        &self,
        names: &[String],
        start_of: impl Fn(&str) -> i32,
        limit: i32,
        all: bool,
    ) -> DescribeTopicPartitionsResponse {
        let node = BrokerId(self.config.node_id);
        let mut resp = DescribeTopicPartitionsResponse::default();
        let mut remaining = limit;
        for name in names {
            if remaining == 0 {
                resp.next_cursor = Some(cursor(name, 0));
                break;
            }
            let topic = TopicName::parse(name)
                .ok()
                .and_then(|n| self.meta.topic(&n));
            let Some(topic) = topic else {
                if !all {
                    let error = if valid_topic_name(name) {
                        ResponseError::UnknownTopicOrPartition
                    } else {
                        ResponseError::InvalidTopicException
                    };
                    resp.topics.push(entry(name, error.code()));
                }
                continue;
            };
            let count = topic.partitions.get();
            let start = start_of(name);
            let upper = count.min(start.saturating_add(remaining));
            let mut t = entry(name, 0);
            t.topic_id = topic.id.to_uuid();
            t.partitions = (start..upper).map(|p| partition(p, node)).collect();
            let taken = i32::try_from(t.partitions.len()).unwrap_or(i32::MAX);
            resp.topics.push(t);
            if upper < count {
                resp.next_cursor = Some(cursor(name, upper));
                break;
            }
            remaining -= taken;
        }
        resp
    }
}

fn partition(index: i32, node: BrokerId) -> DescribeTopicPartitionsResponsePartition {
    let mut p = DescribeTopicPartitionsResponsePartition::default();
    p.partition_index = index;
    p.leader_id = node;
    p.leader_epoch = 0;
    p.replica_nodes = vec![node];
    p.isr_nodes = vec![node];
    p.eligible_leader_replicas = Some(Vec::new());
    p.last_known_elr = Some(Vec::new());
    p
}

/// A topic's entry with no partitions yet: the error, the name, a zero id, and whether Kafka keeps the name for itself.
fn entry(name: &str, error_code: i16) -> DescribeTopicPartitionsResponseTopic {
    let mut t = DescribeTopicPartitionsResponseTopic::default();
    t.error_code = error_code;
    t.name = Some(WireName(StrBytes::from_string(name.to_owned())));
    t.is_internal = INTERNAL.contains(&name);
    t
}

fn cursor(name: &str, partition: i32) -> Cursor {
    let mut c = Cursor::default();
    c.topic_name = WireName(StrBytes::from_string(name.to_owned()));
    c.partition_index = partition;
    c
}

/// Kafka's answer to a request it throws INVALID_REQUEST for (`DescribeTopicPartitionsRequest.getErrorResponse`): every topic
/// named, as named, with the error and nothing else.
fn invalid_request(req: &DescribeTopicPartitionsRequest) -> DescribeTopicPartitionsResponse {
    let mut resp = DescribeTopicPartitionsResponse::default();
    resp.topics = (req.topics.iter())
        .map(|t| {
            let mut e = DescribeTopicPartitionsResponseTopic::default();
            e.error_code = ResponseError::InvalidRequest.code();
            e.name = Some(t.name.clone());
            e
        })
        .collect();
    resp
}

/// A name ordered as Java's `String.compareTo` orders it: by UTF-16 code unit.
#[derive(PartialEq, Eq)]
struct JavaOrdered(String);

impl PartialOrd for JavaOrdered {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for JavaOrdered {
    fn cmp(&self, other: &Self) -> Ordering {
        java_cmp(&self.0, &other.0)
    }
}

fn java_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_ordered_by_utf16_code_unit_as_java_orders_them() {
        // U+FF61 is one unit above every surrogate; U+1F600 is two units starting at a surrogate (0xD83D).
        assert_eq!(java_cmp("\u{1F600}", "\u{FF61}"), Ordering::Less);
        assert_eq!("\u{1F600}".cmp("\u{FF61}"), Ordering::Greater);
        assert_eq!(java_cmp("a", "b"), Ordering::Less);
        assert_eq!(java_cmp("", "a"), Ordering::Less);
    }
}
