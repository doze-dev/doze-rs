//! The requests the steps send, and what is normalised in the answers.

use kafka_protocol::messages::alter_client_quotas_request::{
    EntityData as AlterEntity, EntryData as AlterEntry, OpData,
};
use kafka_protocol::messages::create_topics_request::CreatableTopic;
use kafka_protocol::messages::describe_client_quotas_request::ComponentData;
use kafka_protocol::messages::describe_log_dirs_request::DescribableLogDirTopic;
use kafka_protocol::messages::offset_for_leader_epoch_request::{
    OffsetForLeaderPartition, OffsetForLeaderTopic,
};
use kafka_protocol::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_protocol::messages::{
    AlterClientQuotasRequest, AlterClientQuotasResponse, BrokerId, CreateTopicsRequest,
    DescribeClientQuotasRequest, DescribeClientQuotasResponse, DescribeLogDirsRequest,
    DescribeLogDirsResponse, OffsetForLeaderEpochRequest, ProduceRequest, TopicName,
};
use kafka_protocol::protocol::StrBytes;
use kafka_wire::batch::{OneRecord, single_record};

pub(crate) fn s(text: &str) -> StrBytes {
    StrBytes::from_string(text.to_owned())
}

pub(crate) fn topic(name: &str) -> TopicName {
    TopicName(s(name))
}

pub(crate) fn create(names: &[(&str, i32)]) -> CreateTopicsRequest {
    let mut req = CreateTopicsRequest::default();
    req.timeout_ms = 30_000;
    for (name, partitions) in names {
        let mut t = CreatableTopic::default();
        t.name = topic(name);
        t.num_partitions = *partitions;
        t.replication_factor = 1;
        req.topics.push(t);
    }
    req
}

pub(crate) fn produce(name: &str, partition: i32, value: &str) -> ProduceRequest {
    let batch = single_record(&OneRecord {
        timestamp_ms: 1_700_000_000_000,
        key: Some(b"k"),
        value: Some(value.as_bytes()),
        headers: &[],
    });
    let mut p = PartitionProduceData::default();
    p.index = partition;
    p.records = Some(batch);
    let mut t = TopicProduceData::default();
    t.name = topic(name);
    t.partition_data.push(p);
    let mut req = ProduceRequest::default();
    req.acks = -1;
    req.timeout_ms = 30_000;
    req.topic_data.push(t);
    req
}

pub(crate) fn epochs(asks: &[(&str, &[Ask])], replica: i32) -> OffsetForLeaderEpochRequest {
    let mut req = OffsetForLeaderEpochRequest::default();
    req.replica_id = BrokerId(replica);
    for (name, parts) in asks {
        let mut t = OffsetForLeaderTopic::default();
        t.topic = topic(name);
        for &(partition, current, epoch) in *parts {
            let mut p = OffsetForLeaderPartition::default();
            p.partition = partition;
            p.current_leader_epoch = current;
            p.leader_epoch = epoch;
            t.partitions.push(p);
        }
        req.topics.push(t);
    }
    req
}

pub(crate) fn log_dirs(asks: Option<&[(&str, &[i32])]>) -> DescribeLogDirsRequest {
    let mut req = DescribeLogDirsRequest::default();
    req.topics = asks.map(|asks| {
        asks.iter()
            .map(|(name, parts)| {
                let mut t = DescribableLogDirTopic::default();
                t.topic = topic(name);
                t.partitions = parts.to_vec();
                t
            })
            .collect()
    });
    req
}

/// The path and the volume differ by nature; topics other than this run's are Kafka's own.
pub(crate) fn norm_log_dirs(r: &mut DescribeLogDirsResponse) {
    for dir in &mut r.results {
        dir.log_dir = s("<log dir>");
        dir.total_bytes = dir.total_bytes.signum();
        dir.usable_bytes = dir.usable_bytes.signum();
        dir.topics.retain(|t| t.name.0.starts_with("oracle-"));
        dir.topics.sort_by(|a, b| a.name.0.cmp(&b.name.0));
        for t in &mut dir.topics {
            t.partitions.sort_by_key(|p| p.partition_index);
        }
    }
}

pub(crate) type Entity<'a> = &'a [(&'a str, Option<&'a str>)];
/// One quota key set to a value or removed (`None`).
pub(crate) type Op<'a> = (&'a str, Option<f64>);
/// One entity's alteration.
pub(crate) type Change<'a> = (Entity<'a>, &'a [Op<'a>]);
/// (partition, current leader epoch, epoch asked about)
pub(crate) type Ask = (i32, i32, i32);
/// A DescribeClientQuotas filter: its name, its components (type, match type, name) and whether it is strict.
pub(crate) type FilterCase<'a> = (&'a str, &'a [(&'a str, i8, Option<&'a str>)], bool);

pub(crate) fn alter(entries: &[Change<'_>], validate: bool) -> AlterClientQuotasRequest {
    let mut req = AlterClientQuotasRequest::default();
    req.validate_only = validate;
    for (entity, ops) in entries {
        let mut e = AlterEntry::default();
        e.entity = entity
            .iter()
            .map(|(t, n)| {
                let mut part = AlterEntity::default();
                part.entity_type = s(t);
                part.entity_name = n.map(s);
                part
            })
            .collect();
        e.ops = ops
            .iter()
            .map(|(key, value)| {
                let mut op = OpData::default();
                op.key = s(key);
                op.value = value.unwrap_or(0.0);
                op.remove = value.is_none();
                op
            })
            .collect();
        req.entries.push(e);
    }
    req
}

/// Kafka answers from hash maps: the order of entries, of an entity's parts and of the values is not a contract.
pub(crate) fn norm_alter(r: &mut AlterClientQuotasResponse) {
    for e in &mut r.entries {
        e.entity.sort_by(|a, b| a.entity_type.cmp(&b.entity_type));
    }
    r.entries.sort_by_key(|e| format!("{:?}", e.entity));
}

pub(crate) fn norm_describe(r: &mut DescribeClientQuotasResponse) {
    if let Some(entries) = &mut r.entries {
        for e in entries.iter_mut() {
            e.entity.sort_by(|a, b| a.entity_type.cmp(&b.entity_type));
            e.values.sort_by(|a, b| a.key.cmp(&b.key));
        }
        entries.sort_by_key(|e| format!("{:?}", e.entity));
    }
}

pub(crate) fn describe(
    components: &[(&str, i8, Option<&str>)],
    strict: bool,
) -> DescribeClientQuotasRequest {
    let mut req = DescribeClientQuotasRequest::default();
    req.strict = strict;
    req.components = components
        .iter()
        .map(|(t, m, name)| {
            let mut c = ComponentData::default();
            c.entity_type = s(t);
            c.match_type = *m;
            c._match = name.map(s);
            c
        })
        .collect();
    req
}
