//! The answer a failed request gets: the shape of the API's own response with one error code in every place a client looks for it.
//!
//! One typed function per API the lab can fail, and one match over every request with no wildcard: an API added to the broker
//! without a decision here does not compile. A request the lab cannot answer in the API's own shape (the log-in APIs, the admin
//! APIs, anything whose failure would not be one code) is refused when a rule is made, never when a request arrives.

use kafka_wire::api::{Request, Response};
use kafka_wire::messages::{
    ApiKey, ConsumerGroupHeartbeatResponse, FetchResponse, FindCoordinatorResponse,
    HeartbeatResponse, InitProducerIdResponse, JoinGroupResponse, ListOffsetsResponse,
    MetadataResponse, OffsetCommitResponse, OffsetFetchResponse, ProduceResponse,
    SyncGroupResponse, fetch_response::FetchableTopicResponse, fetch_response::PartitionData,
    find_coordinator_response::Coordinator, list_offsets_response::ListOffsetsPartitionResponse,
    list_offsets_response::ListOffsetsTopicResponse, metadata_response::MetadataResponseTopic,
    offset_commit_response::OffsetCommitResponsePartition,
    offset_commit_response::OffsetCommitResponseTopic,
    offset_fetch_response::OffsetFetchResponseGroup,
    offset_fetch_response::OffsetFetchResponsePartition,
    offset_fetch_response::OffsetFetchResponsePartitions,
    offset_fetch_response::OffsetFetchResponseTopic,
    offset_fetch_response::OffsetFetchResponseTopics, produce_response::PartitionProduceResponse,
    produce_response::TopicProduceResponse,
};

/// The APIs a rule may fail, in the order the lab lists them.
pub const FAILABLE: &[ApiKey] = &[
    ApiKey::Produce,
    ApiKey::Fetch,
    ApiKey::Metadata,
    ApiKey::ListOffsets,
    ApiKey::FindCoordinator,
    ApiKey::JoinGroup,
    ApiKey::SyncGroup,
    ApiKey::Heartbeat,
    ApiKey::OffsetCommit,
    ApiKey::OffsetFetch,
    ApiKey::ConsumerGroupHeartbeat,
    ApiKey::InitProducerId,
];

/// Whether the lab can answer `api` with an error code.
#[must_use]
pub fn can_fail(api: ApiKey) -> bool {
    FAILABLE.contains(&api)
}

/// The response to `request` (sent at `version`) that carries `code` everywhere a client reads one, or `None` for an API the lab
/// cannot answer ([`can_fail`] says which).
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one match over every API with no wildcard, so that an API added to the broker is decided here"
)]
pub fn fail_response(request: &Request, version: i16, code: i16) -> Option<Response> {
    Some(match request {
        Request::Produce(r) => Response::Produce(produce(r, code)),
        Request::Fetch(r) => Response::Fetch(fetch(r, code)),
        Request::Metadata(r) => Response::Metadata(metadata(r, version, code)),
        Request::ListOffsets(r) => Response::ListOffsets(list_offsets(r, code)),
        Request::FindCoordinator(r) => Response::FindCoordinator(coordinator(r, version, code)),
        Request::JoinGroup(_) => Response::JoinGroup(only::<JoinGroupResponse>(code, |r, c| {
            r.error_code = c;
            r.generation_id = -1;
        })),
        Request::SyncGroup(_) => Response::SyncGroup(only::<SyncGroupResponse>(code, |r, c| {
            r.error_code = c;
        })),
        Request::Heartbeat(_) => Response::Heartbeat(only::<HeartbeatResponse>(code, |r, c| {
            r.error_code = c;
        })),
        Request::OffsetCommit(r) => Response::OffsetCommit(offset_commit(r, code)),
        Request::OffsetFetch(r) => Response::OffsetFetch(offset_fetch(r, version, code)),
        Request::ConsumerGroupHeartbeat(_) => {
            Response::ConsumerGroupHeartbeat(only::<ConsumerGroupHeartbeatResponse>(
                code,
                |r, c| {
                    r.error_code = c;
                    r.member_epoch = -1;
                },
            ))
        }
        Request::InitProducerId(_) => {
            Response::InitProducerId(only::<InitProducerIdResponse>(code, |r, c| {
                r.error_code = c;
                r.producer_id = kafka_wire::messages::ProducerId(-1);
                r.producer_epoch = -1;
            }))
        }
        Request::ApiVersions(_)
        | Request::SaslHandshake(_)
        | Request::SaslAuthenticate(_)
        | Request::LeaveGroup(_)
        | Request::DescribeGroups(_)
        | Request::ListGroups(_)
        | Request::CreateTopics(_)
        | Request::DeleteTopics(_)
        | Request::DeleteRecords(_)
        | Request::DeleteGroups(_)
        | Request::OffsetDelete(_)
        | Request::AddPartitionsToTxn(_)
        | Request::AddOffsetsToTxn(_)
        | Request::EndTxn(_)
        | Request::TxnOffsetCommit(_)
        | Request::DescribeAcls(_)
        | Request::CreateAcls(_)
        | Request::DeleteAcls(_)
        | Request::DescribeConfigs(_)
        | Request::AlterConfigs(_)
        | Request::IncrementalAlterConfigs(_)
        | Request::CreatePartitions(_)
        | Request::DescribeCluster(_)
        | Request::DescribeProducers(_)
        | Request::DescribeTransactions(_)
        | Request::ListTransactions(_)
        | Request::ConsumerGroupDescribe(_)
        | Request::DescribeUserScramCredentials(_)
        | Request::AlterUserScramCredentials(_)
        | Request::GetTelemetrySubscriptions(_)
        | Request::PushTelemetry(_) => return None,
    })
}

/// A response that is one code and nothing else, made by `set`.
fn only<T: Default>(code: i16, set: impl FnOnce(&mut T, i16)) -> T {
    let mut r = T::default();
    set(&mut r, code);
    r
}

fn produce(r: &kafka_wire::messages::ProduceRequest, code: i16) -> ProduceResponse {
    let mut resp = ProduceResponse::default();
    for t in &r.topic_data {
        let mut topic = TopicProduceResponse::default();
        topic.name = t.name.clone();
        topic.topic_id = t.topic_id;
        for p in &t.partition_data {
            let mut part = PartitionProduceResponse::default();
            part.index = p.index;
            part.error_code = code;
            part.base_offset = -1;
            part.log_append_time_ms = -1;
            part.log_start_offset = -1;
            topic.partition_responses.push(part);
        }
        resp.responses.push(topic);
    }
    resp
}

fn fetch(r: &kafka_wire::messages::FetchRequest, code: i16) -> FetchResponse {
    let mut resp = FetchResponse::default();
    // The top-level code is Kafka's own for a request refused as a whole (v7 and later); the partitions carry it too, which is
    // where the clients of every version look.
    resp.error_code = 0;
    for t in &r.topics {
        let mut topic = FetchableTopicResponse::default();
        topic.topic = t.topic.clone();
        topic.topic_id = t.topic_id;
        for p in &t.partitions {
            let mut part = PartitionData::default();
            part.partition_index = p.partition;
            part.error_code = code;
            part.high_watermark = -1;
            part.last_stable_offset = -1;
            part.log_start_offset = -1;
            topic.partitions.push(part);
        }
        resp.responses.push(topic);
    }
    resp
}

fn metadata(
    r: &kafka_wire::messages::MetadataRequest,
    version: i16,
    code: i16,
) -> MetadataResponse {
    let mut resp = MetadataResponse::default();
    // A code for the whole answer exists from v13; before it, the topics' own are all there is.
    if version >= 13 {
        resp.error_code = code;
    }
    for t in r.topics.iter().flatten() {
        let mut topic = MetadataResponseTopic::default();
        topic.error_code = code;
        topic.name = t.name.clone();
        topic.topic_id = t.topic_id;
        resp.topics.push(topic);
    }
    resp
}

fn list_offsets(r: &kafka_wire::messages::ListOffsetsRequest, code: i16) -> ListOffsetsResponse {
    let mut resp = ListOffsetsResponse::default();
    for t in &r.topics {
        let mut topic = ListOffsetsTopicResponse::default();
        topic.name = t.name.clone();
        for p in &t.partitions {
            let mut part = ListOffsetsPartitionResponse::default();
            part.partition_index = p.partition_index;
            part.error_code = code;
            part.timestamp = -1;
            part.offset = -1;
            topic.partitions.push(part);
        }
        resp.topics.push(topic);
    }
    resp
}

/// FindCoordinator names its coordinators from v4: the code goes in each of them; before, in the one at the top.
fn coordinator(
    r: &kafka_wire::messages::FindCoordinatorRequest,
    version: i16,
    code: i16,
) -> FindCoordinatorResponse {
    let mut resp = FindCoordinatorResponse::default();
    if version < 4 {
        resp.error_code = code;
        resp.node_id = kafka_wire::messages::BrokerId(-1);
        resp.port = -1;
        return resp;
    }
    for key in &r.coordinator_keys {
        let mut c = Coordinator::default();
        c.key = key.clone();
        c.node_id = kafka_wire::messages::BrokerId(-1);
        c.port = -1;
        c.error_code = code;
        resp.coordinators.push(c);
    }
    resp
}

fn offset_commit(r: &kafka_wire::messages::OffsetCommitRequest, code: i16) -> OffsetCommitResponse {
    let mut resp = OffsetCommitResponse::default();
    for t in &r.topics {
        let mut topic = OffsetCommitResponseTopic::default();
        topic.name = t.name.clone();
        topic.topic_id = t.topic_id;
        for p in &t.partitions {
            let mut part = OffsetCommitResponsePartition::default();
            part.partition_index = p.partition_index;
            part.error_code = code;
            topic.partitions.push(part);
        }
        resp.topics.push(topic);
    }
    resp
}

/// OffsetFetch names its groups from v8: the code goes in each group, and in the old shape on every partition and at the top.
fn offset_fetch(
    r: &kafka_wire::messages::OffsetFetchRequest,
    version: i16,
    code: i16,
) -> OffsetFetchResponse {
    let mut resp = OffsetFetchResponse::default();
    if version >= 8 {
        for g in &r.groups {
            let mut group = OffsetFetchResponseGroup::default();
            group.group_id = g.group_id.clone();
            group.error_code = code;
            for t in g.topics.iter().flatten() {
                let mut topic = OffsetFetchResponseTopics::default();
                topic.name = t.name.clone();
                topic.topic_id = t.topic_id;
                for p in &t.partition_indexes {
                    let mut part = OffsetFetchResponsePartitions::default();
                    part.partition_index = *p;
                    part.committed_offset = -1;
                    part.committed_leader_epoch = -1;
                    part.error_code = code;
                    topic.partitions.push(part);
                }
                group.topics.push(topic);
            }
            resp.groups.push(group);
        }
        return resp;
    }
    resp.error_code = code;
    for t in r.topics.iter().flatten() {
        let mut topic = OffsetFetchResponseTopic::default();
        topic.name = t.name.clone();
        for p in &t.partition_indexes {
            let mut part = OffsetFetchResponsePartition::default();
            part.partition_index = *p;
            part.committed_offset = -1;
            part.committed_leader_epoch = -1;
            part.error_code = code;
            topic.partitions.push(part);
        }
        resp.topics.push(topic);
    }
    resp
}

#[cfg(test)]
mod tests;
