//! The share group APIs' cases (KIP-932): a member's heartbeat, a fetch that acquired records and carried acknowledgements, an
//! acknowledgement, and the admin calls.

use kafka_wire::api::{Request, Response};
use kafka_wire::messages::*;

use super::cases::{batch, group, named, s, topic};

pub(super) fn share(out: &mut Vec<(ApiKey, Request, Response)>) {
    macro_rules! case {
        ($key:ident, $req:expr, $resp:expr) => {
            out.push((ApiKey::$key, Request::$key($req), Response::$key($resp)));
        };
    }
    case!(
        ShareGroupHeartbeat,
        named(|r: &mut ShareGroupHeartbeatRequest| {
            r.group_id = group("emails");
            r.member_id = s("m");
            r.subscribed_topic_names = Some(vec![topic("orders")]);
        }),
        named(|r: &mut ShareGroupHeartbeatResponse| {
            r.member_epoch = 2;
            r.assignment = Some(named(
                |a: &mut share_group_heartbeat_response::Assignment| {
                    a.topic_partitions = vec![named(
                        |t: &mut share_group_heartbeat_response::TopicPartitions| {
                            t.partitions = vec![0, 1, 2]
                        },
                    )];
                },
            ));
        })
    );
    case!(
        ShareGroupDescribe,
        named(|r: &mut ShareGroupDescribeRequest| r.group_ids = vec![group("emails")]),
        ShareGroupDescribeResponse::default()
    );
    case!(
        ShareFetch,
        named(|r: &mut ShareFetchRequest| {
            r.group_id = Some(group("emails"));
            r.member_id = Some(s("m"));
            r.share_session_epoch = 1;
            r.max_wait_ms = 500;
            r.topics = vec![named(|t: &mut share_fetch_request::FetchTopic| {
                t.partitions = vec![named(|p: &mut share_fetch_request::FetchPartition| {
                    p.acknowledgement_batches = vec![named(
                        |b: &mut share_fetch_request::AcknowledgementBatch| {
                            b.last_offset = 4;
                            b.acknowledge_types = vec![1];
                        },
                    )];
                })];
            })];
        }),
        named(|r: &mut ShareFetchResponse| {
            r.responses = vec![named(
                |t: &mut share_fetch_response::ShareFetchableTopicResponse| {
                    t.partitions = vec![named(|p: &mut share_fetch_response::PartitionData| {
                        p.records = Some(batch());
                        p.acquired_records =
                            vec![named(|a: &mut share_fetch_response::AcquiredRecords| {
                                a.first_offset = 5;
                                a.last_offset = 7;
                                a.delivery_count = 1;
                            })];
                    })];
                },
            )];
        })
    );
    case!(
        ShareAcknowledge,
        named(|r: &mut ShareAcknowledgeRequest| {
            r.group_id = Some(group("emails"));
            r.member_id = Some(s("m"));
            r.share_session_epoch = 2;
            r.topics = vec![named(
                |t: &mut share_acknowledge_request::AcknowledgeTopic| {
                    t.partitions = vec![named(
                        |p: &mut share_acknowledge_request::AcknowledgePartition| {
                            p.acknowledgement_batches = vec![named(
                                |b: &mut share_acknowledge_request::AcknowledgementBatch| {
                                    b.first_offset = 5;
                                    b.last_offset = 7;
                                    b.acknowledge_types = vec![1, 2, 3];
                                },
                            )];
                        },
                    )];
                },
            )];
        }),
        ShareAcknowledgeResponse::default()
    );
    case!(
        DescribeShareGroupOffsets,
        named(|r: &mut DescribeShareGroupOffsetsRequest| {
            r.groups = vec![named(
                |g: &mut describe_share_group_offsets_request::DescribeShareGroupOffsetsRequestGroup| g.group_id = group("emails"),
            )];
        }),
        DescribeShareGroupOffsetsResponse::default()
    );
    case!(
        AlterShareGroupOffsets,
        named(|r: &mut AlterShareGroupOffsetsRequest| r.group_id = group("emails")),
        AlterShareGroupOffsetsResponse::default()
    );
    case!(
        DeleteShareGroupOffsets,
        named(|r: &mut DeleteShareGroupOffsetsRequest| r.group_id = group("emails")),
        DeleteShareGroupOffsetsResponse::default()
    );
}
