//! DescribeProducers: the producers active on a topic's partitions (the count on the topic page's tab).

use kafka_wire::StrBytes;
use kafka_wire::api::Request;
use kafka_wire::messages::DescribeProducersRequest;
use kafka_wire::messages::TopicName;
use kafka_wire::messages::describe_producers_request::TopicRequest;

/// The producers of these partitions of a topic. At most 1,024 partitions.
#[must_use]
pub fn describe_producers(topic: &str, partitions: &[i32]) -> Request {
    let mut t = TopicRequest::default();
    t.name = TopicName(StrBytes::from_string(topic.to_owned()));
    t.partition_indexes = partitions.iter().copied().take(1024).collect();
    let mut r = DescribeProducersRequest::default();
    r.topics = vec![t];
    Request::DescribeProducers(r)
}
