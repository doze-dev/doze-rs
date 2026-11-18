//! Metadata, topic administration and the cluster description.

use kafka_wire::messages::{
    CreatePartitionsRequest, CreatePartitionsResponse, CreateTopicsRequest, CreateTopicsResponse,
    DeleteRecordsRequest, DeleteRecordsResponse, DeleteTopicsRequest, DeleteTopicsResponse,
    DescribeClusterRequest, DescribeClusterResponse, MetadataRequest, MetadataResponse,
};

use super::n;
use crate::recorder::facts::Facts;

pub(super) fn metadata_request(r: &MetadataRequest, f: &mut Facts) {
    match &r.topics {
        None => f.about("all topics"),
        Some(topics) => {
            for t in topics {
                match &t.name {
                    Some(name) => f.topic(name.0.as_ref(), []),
                    None => f.topic(&t.topic_id.to_string(), []),
                }
            }
            f.asked = n(topics.len());
        }
    }
}

pub(super) fn metadata_response(r: &MetadataResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.items = n(r.topics.len());
    for t in &r.topics {
        f.error(t.error_code);
    }
}

pub(super) fn create_request(r: &CreateTopicsRequest, f: &mut Facts) {
    for t in &r.topics {
        f.topic(&t.name.0, []);
    }
    f.asked = n(r.topics.len());
    if r.validate_only {
        f.about("validate only");
    }
}

pub(super) fn create_response(r: &CreateTopicsResponse, f: &mut Facts) {
    for t in &r.topics {
        f.result(&t.name.0, -1, t.error_code, None);
        f.say(t.error_message.as_deref());
    }
    f.items = n(r.topics.len());
}

pub(super) fn delete_request(r: &DeleteTopicsRequest, f: &mut Facts) {
    for t in &r.topics {
        match &t.name {
            Some(name) => f.topic(&name.0, []),
            None => f.topic(&t.topic_id.to_string(), []),
        }
    }
    for name in &r.topic_names {
        f.topic(&name.0, []);
    }
    f.asked = n(r.topics.len() + r.topic_names.len());
}

pub(super) fn delete_response(r: &DeleteTopicsResponse, f: &mut Facts) {
    for t in &r.responses {
        let name = t
            .name
            .as_ref()
            .map_or_else(|| t.topic_id.to_string(), |n| n.0.to_string());
        f.result(&name, -1, t.error_code, None);
        f.say(t.error_message.as_deref());
    }
    f.items = n(r.responses.len());
}

pub(super) fn delete_records_request(r: &DeleteRecordsRequest, f: &mut Facts) {
    for t in &r.topics {
        f.topic(&t.name.0, t.partitions.iter().map(|p| p.partition_index));
    }
}

pub(super) fn delete_records_response(r: &DeleteRecordsResponse, f: &mut Facts) {
    for t in &r.topics {
        for p in &t.partitions {
            f.result(
                &t.name.0,
                p.partition_index,
                p.error_code,
                (p.error_code == 0).then_some(p.low_watermark),
            );
        }
    }
}

pub(super) fn partitions_request(r: &CreatePartitionsRequest, f: &mut Facts) {
    for t in &r.topics {
        f.topic(&t.name.0, []);
    }
    f.asked = n(r.topics.len());
    if r.validate_only {
        f.about("validate only");
    }
}

pub(super) fn partitions_response(r: &CreatePartitionsResponse, f: &mut Facts) {
    for t in &r.results {
        f.result(&t.name.0, -1, t.error_code, None);
        f.say(t.error_message.as_deref());
    }
}

pub(super) fn cluster_request(_: &DescribeClusterRequest, f: &mut Facts) {
    f.about("the cluster");
}

pub(super) fn cluster_response(r: &DescribeClusterResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.say(r.error_message.as_deref());
    f.items = n(r.brokers.len());
}
