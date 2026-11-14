//! One representative request and answer for every served API.

use bytes::Bytes;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::batch::build::BatchBuilder;
use kafka_wire::messages::*;

pub(super) fn s(text: &str) -> StrBytes {
    StrBytes::from_string(text.to_owned())
}
pub(super) fn topic(text: &str) -> TopicName {
    TopicName(s(text))
}
pub(super) fn group(text: &str) -> GroupId {
    GroupId(s(text))
}
pub(super) fn txn(text: &str) -> TransactionalId {
    TransactionalId(s(text))
}

/// A batch of three records from producer 4017 at epoch 2, in a transaction.
pub(super) fn batch() -> Bytes {
    BatchBuilder::new()
        .records(3)
        .producer(4017, 2, 0)
        .attributes(0x10)
        .body(b"PLANTED-RECORD-VALUE".to_vec())
        .build()
}

pub(super) fn produce() -> (Request, Response) {
    let mut p = produce_request::PartitionProduceData::default();
    p.index = 0;
    p.records = Some(batch());
    let mut t = produce_request::TopicProduceData::default();
    t.name = topic("orders");
    t.partition_data = vec![p];
    let mut q = ProduceRequest::default();
    q.acks = -1;
    q.transactional_id = Some(txn("orders-eos"));
    q.topic_data = vec![t];
    let mut pr = produce_response::PartitionProduceResponse::default();
    pr.index = 0;
    pr.error_code = 87;
    pr.error_message = Some(s(
        "Compacted topic cleanup.policy=compact requires every record to have a key",
    ));
    let mut tr = produce_response::TopicProduceResponse::default();
    tr.name = topic("orders");
    tr.partition_responses = vec![pr];
    let mut a = ProduceResponse::default();
    a.responses = vec![tr];
    (Request::Produce(q), Response::Produce(a))
}

pub(super) fn fetch() -> (Request, Response) {
    let mut p = fetch_request::FetchPartition::default();
    p.partition = 1;
    p.fetch_offset = 7402;
    let mut t = fetch_request::FetchTopic::default();
    t.topic = topic("payments.settled");
    t.partitions = vec![p];
    let mut q = FetchRequest::default();
    q.max_wait_ms = 500;
    q.isolation_level = 1;
    q.topics = vec![t];
    let mut pd = fetch_response::PartitionData::default();
    pd.partition_index = 1;
    pd.high_watermark = 7500;
    pd.records = Some(batch());
    let mut tr = fetch_response::FetchableTopicResponse::default();
    tr.topic = topic("payments.settled");
    tr.partitions = vec![pd];
    let mut a = FetchResponse::default();
    a.responses = vec![tr];
    (Request::Fetch(q), Response::Fetch(a))
}

pub(super) fn named<T: Default>(set: impl FnOnce(&mut T)) -> T {
    let mut v = T::default();
    set(&mut v);
    v
}

/// One representative pair for every served API, keyed by the API.
#[allow(clippy::too_many_lines)] // a table of cases: one line of data per API
pub(super) fn cases() -> Vec<(ApiKey, Request, Response)> {
    let (pq, pa) = produce();
    let (fq, fa) = fetch();
    let mut out = vec![(ApiKey::Produce, pq, pa), (ApiKey::Fetch, fq, fa)];
    macro_rules! case {
        ($key:ident, $req:expr, $resp:expr) => {
            out.push((ApiKey::$key, Request::$key($req), Response::$key($resp)));
        };
    }
    case!(
        ListOffsets,
        named(|r: &mut ListOffsetsRequest| {
            r.topics = vec![named(|t: &mut list_offsets_request::ListOffsetsTopic| {
                t.name = topic("orders");
                t.partitions = vec![named(
                    |p: &mut list_offsets_request::ListOffsetsPartition| p.partition_index = 2,
                )];
            })];
        }),
        named(|r: &mut ListOffsetsResponse| {
            r.topics = vec![named(
                |t: &mut list_offsets_response::ListOffsetsTopicResponse| {
                    t.name = topic("orders");
                    t.partitions = vec![named(
                        |p: &mut list_offsets_response::ListOffsetsPartitionResponse| {
                            p.partition_index = 2;
                            p.offset = 16092;
                        },
                    )];
                },
            )];
        })
    );
    case!(
        Metadata,
        MetadataRequest::default(),
        named(|r: &mut MetadataResponse| {
            r.topics = vec![named(|t: &mut metadata_response::MetadataResponseTopic| {
                t.name = Some(topic("orders"))
            })];
        })
    );
    case!(
        OffsetCommit,
        named(|r: &mut OffsetCommitRequest| {
            r.group_id = group("payments-workers");
            r.member_id = s("m-1");
            r.generation_id_or_member_epoch = 4;
            r.topics = vec![named(
                |t: &mut offset_commit_request::OffsetCommitRequestTopic| {
                    t.name = topic("payments.settled");
                    t.partitions = vec![named(
                        |p: &mut offset_commit_request::OffsetCommitRequestPartition| {
                            p.committed_metadata = Some(s("PLANTED-METADATA"))
                        },
                    )];
                },
            )];
        }),
        named(|r: &mut OffsetCommitResponse| {
            r.topics = vec![named(
                |t: &mut offset_commit_response::OffsetCommitResponseTopic| {
                    t.name = topic("payments.settled");
                    t.partitions = vec![named(
                        |p: &mut offset_commit_response::OffsetCommitResponsePartition| {
                            p.error_code = 22
                        },
                    )];
                },
            )];
        })
    );
    case!(
        OffsetFetch,
        named(|r: &mut OffsetFetchRequest| r.group_id = group("search-indexer")),
        OffsetFetchResponse::default()
    );
    case!(
        FindCoordinator,
        named(|r: &mut FindCoordinatorRequest| r.key = s("fulfilment")),
        named(|r: &mut FindCoordinatorResponse| r.error_code = 15)
    );
    case!(
        JoinGroup,
        named(|r: &mut JoinGroupRequest| {
            r.group_id = group("fulfilment");
            r.member_id = s("fulfilment-py-2");
            r.reason = Some(s("rebalance: a member joined"));
        }),
        named(|r: &mut JoinGroupResponse| {
            r.generation_id = 30;
            r.member_id = s("fulfilment-py-2");
        })
    );
    case!(
        Heartbeat,
        named(|r: &mut HeartbeatRequest| {
            r.group_id = group("fulfilment");
            r.member_id = s("fulfilment-py-1");
            r.generation_id = 30;
        }),
        named(|r: &mut HeartbeatResponse| r.error_code = 27)
    );
    case!(
        LeaveGroup,
        named(|r: &mut LeaveGroupRequest| {
            r.group_id = group("fulfilment");
            r.members = vec![named(|m: &mut leave_group_request::MemberIdentity| {
                m.member_id = s("fulfilment-py-2");
                m.reason = Some(s("leaving"));
            })];
        }),
        LeaveGroupResponse::default()
    );
    case!(
        SyncGroup,
        named(|r: &mut SyncGroupRequest| {
            r.group_id = group("fulfilment");
            r.member_id = s("fulfilment-py-1");
            r.generation_id = 30;
        }),
        SyncGroupResponse::default()
    );
    case!(
        DescribeGroups,
        named(|r: &mut DescribeGroupsRequest| r.groups = vec![group("fulfilment")]),
        DescribeGroupsResponse::default()
    );
    case!(
        ListGroups,
        ListGroupsRequest::default(),
        ListGroupsResponse::default()
    );
    case!(
        SaslHandshake,
        named(|r: &mut SaslHandshakeRequest| r.mechanism = s("SCRAM-SHA-256")),
        SaslHandshakeResponse::default()
    );
    case!(
        ApiVersions,
        named(|r: &mut ApiVersionsRequest| {
            r.client_software_name = s("librdkafka");
            r.client_software_version = s("2.3.0");
        }),
        ApiVersionsResponse::default()
    );
    case!(
        CreateTopics,
        named(|r: &mut CreateTopicsRequest| {
            r.topics = vec![named(|t: &mut create_topics_request::CreatableTopic| {
                t.name = topic("clickstream");
                t.configs = vec![named(
                    |c: &mut create_topics_request::CreatableTopicConfig| {
                        c.name = s("sasl.jaas.config");
                        c.value = Some(s("PLANTED-CONFIG-VALUE"));
                    },
                )];
            })];
        }),
        named(|r: &mut CreateTopicsResponse| {
            r.topics = vec![named(
                |t: &mut create_topics_response::CreatableTopicResult| {
                    t.name = topic("clickstream");
                    t.error_code = 36;
                },
            )];
        })
    );
    case!(
        DeleteTopics,
        named(|r: &mut DeleteTopicsRequest| r.topic_names = vec![topic("old")]),
        DeleteTopicsResponse::default()
    );
    case!(
        DeleteRecords,
        named(|r: &mut DeleteRecordsRequest| {
            r.topics = vec![named(
                |t: &mut delete_records_request::DeleteRecordsTopic| t.name = topic("orders"),
            )];
        }),
        DeleteRecordsResponse::default()
    );
    case!(
        InitProducerId,
        named(|r: &mut InitProducerIdRequest| r.transactional_id = Some(txn("orders-eos"))),
        named(|r: &mut InitProducerIdResponse| {
            r.producer_id = ProducerId(4017);
            r.producer_epoch = 2;
        })
    );
    case!(
        AddPartitionsToTxn,
        named(|r: &mut AddPartitionsToTxnRequest| {
            r.v3_and_below_transactional_id = txn("orders-eos");
            r.v3_and_below_producer_id = ProducerId(4017);
            r.v3_and_below_producer_epoch = 2;
        }),
        AddPartitionsToTxnResponse::default()
    );
    case!(
        AddOffsetsToTxn,
        named(|r: &mut AddOffsetsToTxnRequest| {
            r.transactional_id = txn("orders-eos");
            r.group_id = group("payments");
        }),
        AddOffsetsToTxnResponse::default()
    );
    case!(
        EndTxn,
        named(|r: &mut EndTxnRequest| {
            r.transactional_id = txn("orders-eos");
            r.committed = true;
        }),
        EndTxnResponse::default()
    );
    case!(
        TxnOffsetCommit,
        named(|r: &mut TxnOffsetCommitRequest| {
            r.transactional_id = txn("orders-eos");
            r.group_id = group("payments");
        }),
        TxnOffsetCommitResponse::default()
    );
    case!(
        DescribeAcls,
        DescribeAclsRequest::default(),
        DescribeAclsResponse::default()
    );
    case!(
        CreateAcls,
        named(|r: &mut CreateAclsRequest| r.creations =
            vec![create_acls_request::AclCreation::default()]),
        CreateAclsResponse::default()
    );
    case!(
        DeleteAcls,
        DeleteAclsRequest::default(),
        DeleteAclsResponse::default()
    );
    super::cases_more::more(&mut out);
    out
}
