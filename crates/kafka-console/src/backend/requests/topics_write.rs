//! The writes of the topic forms: CreateTopics, Produce, CreatePartitions, DeleteRecords and DeleteTopics. Each is the
//! request a Kafka client would send for the same click, built to the shape the handler reads.

use bytes::Bytes;
use kafka_wire::StrBytes;
use kafka_wire::Uuid;
use kafka_wire::api::Request;
use kafka_wire::messages::create_topics_request::{CreatableTopic, CreatableTopicConfig};
use kafka_wire::messages::delete_records_request::{DeleteRecordsPartition, DeleteRecordsTopic};
use kafka_wire::messages::delete_topics_request::DeleteTopicState;
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::{
    CreatePartitionsRequest, CreateTopicsRequest, DeleteRecordsRequest, DeleteTopicsRequest,
    ProduceRequest, TopicName,
};

/// How long the broker may take over a topic-admin call, as the Java admin client's default request timeout.
const TIMEOUT_MS: i32 = 30_000;

fn name(s: &str) -> TopicName {
    TopicName(StrBytes::from_string(s.to_owned()))
}

/// Make one topic with `partitions` partitions, replication factor 1 (the broker is the only replica), and these configs.
/// `validate_only` asks the broker to run every check and create nothing ("Check only").
#[must_use]
pub fn create_topic(
    topic: &str,
    partitions: i32,
    configs: &[(&str, &str)],
    validate_only: bool,
) -> Request {
    let mut t = CreatableTopic::default();
    t.name = name(topic);
    t.num_partitions = partitions;
    t.replication_factor = 1;
    t.configs = configs
        .iter()
        .map(|(k, v)| {
            let mut c = CreatableTopicConfig::default();
            c.name = StrBytes::from_string((*k).to_owned());
            c.value = Some(StrBytes::from_string((*v).to_owned()));
            c
        })
        .collect();
    let mut r = CreateTopicsRequest::default();
    r.topics = vec![t];
    r.timeout_ms = TIMEOUT_MS;
    r.validate_only = validate_only;
    Request::CreateTopics(r)
}

/// Write one batch (as a client frames it) to one partition. The topic is named and, for versions that address it by
/// id, also identified. `acks` is -1, 1 or 0.
#[must_use]
pub fn produce(topic: &str, topic_id: Uuid, partition: i32, acks: i16, batch: Bytes) -> Request {
    let mut p = PartitionProduceData::default();
    p.index = partition;
    p.records = Some(batch);
    let mut t = TopicProduceData::default();
    t.name = name(topic);
    t.topic_id = topic_id;
    t.partition_data = vec![p];
    let mut r = ProduceRequest::default();
    r.transactional_id = None;
    r.acks = acks;
    r.timeout_ms = TIMEOUT_MS;
    r.topic_data = vec![t];
    Request::Produce(r)
}

/// Raise a topic's partition count to `count` (the new total, not the number to add); `validate_only` checks only.
#[must_use]
pub fn create_partitions(topic: &str, count: i32, validate_only: bool) -> Request {
    let mut t = kafka_wire::messages::create_partitions_request::CreatePartitionsTopic::default();
    t.name = name(topic);
    t.count = count;
    t.assignments = None;
    let mut r = CreatePartitionsRequest::default();
    r.topics = vec![t];
    r.timeout_ms = TIMEOUT_MS;
    r.validate_only = validate_only;
    Request::CreatePartitions(r)
}

/// Move a partition's log start to `offset` (-1: the high watermark).
#[must_use]
pub fn delete_records(topic: &str, partition: i32, offset: i64) -> Request {
    let mut p = DeleteRecordsPartition::default();
    p.partition_index = partition;
    p.offset = offset;
    let mut t = DeleteRecordsTopic::default();
    t.name = name(topic);
    t.partitions = vec![p];
    let mut r = DeleteRecordsRequest::default();
    r.topics = vec![t];
    r.timeout_ms = TIMEOUT_MS;
    Request::DeleteRecords(r)
}

/// Delete one topic, by name.
#[must_use]
pub fn delete_topic(topic: &str) -> Request {
    let mut s = DeleteTopicState::default();
    s.name = Some(name(topic));
    let mut r = DeleteTopicsRequest::default();
    r.topics = vec![s];
    r.timeout_ms = TIMEOUT_MS;
    Request::DeleteTopics(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_create_names_the_topic_its_count_one_replica_and_the_configs_in_order() {
        let Request::CreateTopics(r) = create_topic(
            "orders.retry",
            3,
            &[("cleanup.policy", "compact"), ("retention.ms", "-1")],
            true,
        ) else {
            panic!()
        };
        assert!(r.validate_only);
        let t = &r.topics[0];
        assert_eq!(
            (t.name.0.as_str(), t.num_partitions, t.replication_factor),
            ("orders.retry", 3, 1)
        );
        let configs: Vec<_> = t
            .configs
            .iter()
            .map(|c| (c.name.as_str(), c.value.as_ref().map(|v| v.as_str())))
            .collect();
        assert_eq!(
            configs,
            [
                ("cleanup.policy", Some("compact")),
                ("retention.ms", Some("-1"))
            ]
        );
    }

    #[test]
    fn a_produce_carries_one_batch_for_one_partition_with_the_acks_chosen() {
        let Request::Produce(r) = produce("t", Uuid::nil(), 2, -1, Bytes::from_static(b"batch"))
        else {
            panic!()
        };
        assert_eq!((r.acks, r.timeout_ms), (-1, 30_000));
        assert_eq!(r.topic_data.len(), 1);
        let p = &r.topic_data[0].partition_data[0];
        assert_eq!((p.index, p.records.as_deref()), (2, Some(&b"batch"[..])));
        assert!(r.transactional_id.is_none());
    }

    #[test]
    fn the_admin_calls_carry_what_was_asked_and_no_more() {
        let Request::CreatePartitions(r) = create_partitions("orders", 5, false) else {
            panic!()
        };
        assert_eq!(
            (
                r.topics[0].count,
                r.topics[0].assignments.is_none(),
                r.validate_only
            ),
            (5, true, false)
        );
        let Request::DeleteRecords(r) = delete_records("orders", 0, 16_000) else {
            panic!()
        };
        assert_eq!(
            (
                r.topics[0].partitions[0].partition_index,
                r.topics[0].partitions[0].offset
            ),
            (0, 16_000)
        );
        let Request::DeleteTopics(r) = delete_topic("orders") else {
            panic!()
        };
        assert_eq!(r.topics.len(), 1);
        assert!(r.topic_names.is_empty());
        assert_eq!(
            r.topics[0].name.as_ref().map(|n| n.0.as_str()),
            Some("orders")
        );
    }
}
