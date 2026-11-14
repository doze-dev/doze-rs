//! Builders for the tests of this module: requests, heads and a clock that only moves when told.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use kafka_auth::Principal;
use kafka_wire::StrBytes;
use kafka_wire::api::Request;
use kafka_wire::header::RequestHead;
use kafka_wire::messages::{
    ApiKey, FetchRequest, GroupId, HeartbeatRequest, MetadataRequest, ProduceRequest, TopicName,
    fetch_request::{FetchPartition, FetchTopic},
    metadata_request::MetadataRequestTopic,
    produce_request::{PartitionProduceData, TopicProduceData},
};

use super::{Ask, Clock};

/// A clock the test moves.
#[derive(Debug, Clone, Default)]
pub(super) struct Manual(pub(super) Arc<AtomicU64>);

impl Manual {
    pub(super) fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for Manual {
    fn mono_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    fn wall_ms(&self) -> i64 {
        1_700_000_000_000 + i64::try_from(self.mono_ms()).unwrap_or(0)
    }
}

pub(super) fn head(api: ApiKey, version: i16, client: &str) -> RequestHead {
    RequestHead {
        key: api,
        version,
        correlation_id: 7,
        client_id: Some(StrBytes::from_string(client.to_owned())),
    }
}

pub(super) fn produce(topic: &str, partitions: &[i32], acks: i16) -> Request {
    let mut t = TopicProduceData::default();
    t.name = TopicName(StrBytes::from_string(topic.to_owned()));
    for p in partitions {
        let mut d = PartitionProduceData::default();
        d.index = *p;
        t.partition_data.push(d);
    }
    let mut r = ProduceRequest::default();
    r.acks = acks;
    r.topic_data.push(t);
    Request::Produce(r)
}

pub(super) fn fetch(topic: &str, partitions: &[i32]) -> Request {
    let mut t = FetchTopic::default();
    t.topic = TopicName(StrBytes::from_string(topic.to_owned()));
    for p in partitions {
        let mut d = FetchPartition::default();
        d.partition = *p;
        t.partitions.push(d);
    }
    let mut r = FetchRequest::default();
    r.topics.push(t);
    Request::Fetch(r)
}

pub(super) fn metadata(topics: &[&str]) -> Request {
    let mut r = MetadataRequest::default();
    r.topics = Some(
        topics
            .iter()
            .map(|n| {
                let mut t = MetadataRequestTopic::default();
                t.name = Some(TopicName(StrBytes::from_string((*n).to_owned())));
                t
            })
            .collect(),
    );
    Request::Metadata(r)
}

pub(super) fn heartbeat(group: &str) -> Request {
    let mut r = HeartbeatRequest::default();
    r.group_id = GroupId(StrBytes::from_string(group.to_owned()));
    Request::Heartbeat(r)
}

/// A request seen by a rule: everything but the request is owned by the caller.
pub(super) fn ask<'a>(
    conn: Option<u64>,
    principal: &'a Principal,
    head: &'a RequestHead,
    request: Option<&'a Request>,
) -> Ask<'a> {
    Ask {
        conn,
        principal,
        head,
        request,
        names: &|_| None,
    }
}
