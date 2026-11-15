//! The second half of the table of cases.
#![allow(clippy::too_many_lines)] // a table of cases: one line of data per API

use bytes::Bytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::*;

use super::cases::{group, named, s, topic, txn};

pub(super) fn more(out: &mut Vec<(ApiKey, Request, Response)>) {
    macro_rules! case {
        ($key:ident, $req:expr, $resp:expr) => {
            out.push((ApiKey::$key, Request::$key($req), Response::$key($resp)));
        };
    }
    case!(
        DescribeConfigs,
        named(|r: &mut DescribeConfigsRequest| {
            r.resources = vec![named(
                |x: &mut describe_configs_request::DescribeConfigsResource| {
                    x.resource_type = 2;
                    x.resource_name = s("orders");
                },
            )];
        }),
        DescribeConfigsResponse::default()
    );
    case!(
        AlterConfigs,
        named(|r: &mut AlterConfigsRequest| {
            r.resources = vec![named(
                |x: &mut alter_configs_request::AlterConfigsResource| {
                    x.resource_type = 4;
                    x.resource_name = s("0");
                    x.configs = vec![named(|c: &mut alter_configs_request::AlterableConfig| {
                        c.name = s("ssl.key.password");
                        c.value = Some(s("PLANTED-ALTER-VALUE"));
                    })];
                },
            )];
        }),
        AlterConfigsResponse::default()
    );
    case!(
        SaslAuthenticate,
        named(|r: &mut SaslAuthenticateRequest| r.auth_bytes =
            Bytes::from_static(b"\0alice\0PLANTED-PASSWORD")),
        named(|r: &mut SaslAuthenticateResponse| {
            r.error_code = 58;
            r.auth_bytes = Bytes::from_static(b"PLANTED-SERVER-FIRST");
        })
    );
    case!(
        CreatePartitions,
        named(|r: &mut CreatePartitionsRequest| {
            r.topics = vec![named(
                |t: &mut create_partitions_request::CreatePartitionsTopic| {
                    t.name = topic("orders");
                    t.count = 6;
                },
            )];
        }),
        named(|r: &mut CreatePartitionsResponse| {
            r.results = vec![named(
                |t: &mut create_partitions_response::CreatePartitionsTopicResult| {
                    t.name = topic("orders");
                    t.error_code = 37;
                },
            )];
        })
    );
    case!(
        DeleteGroups,
        named(|r: &mut DeleteGroupsRequest| r.groups_names = vec![group("idle")]),
        DeleteGroupsResponse::default()
    );
    case!(
        IncrementalAlterConfigs,
        named(|r: &mut IncrementalAlterConfigsRequest| {
            r.resources = vec![named(
                |x: &mut incremental_alter_configs_request::AlterConfigsResource| {
                    x.resource_type = 2;
                    x.resource_name = s("orders");
                },
            )];
        }),
        IncrementalAlterConfigsResponse::default()
    );
    case!(
        OffsetDelete,
        named(|r: &mut OffsetDeleteRequest| r.group_id = group("idle")),
        OffsetDeleteResponse::default()
    );
    case!(
        DescribeCluster,
        DescribeClusterRequest::default(),
        DescribeClusterResponse::default()
    );
    case!(
        DescribeProducers,
        named(|r: &mut DescribeProducersRequest| {
            r.topics = vec![named(|t: &mut describe_producers_request::TopicRequest| {
                t.name = topic("orders");
                t.partition_indexes = vec![0];
            })];
        }),
        DescribeProducersResponse::default()
    );
    case!(
        DescribeTransactions,
        named(|r: &mut DescribeTransactionsRequest| r.transactional_ids = vec![txn("orders-eos")]),
        DescribeTransactionsResponse::default()
    );
    case!(
        ListTransactions,
        ListTransactionsRequest::default(),
        ListTransactionsResponse::default()
    );
    case!(
        ConsumerGroupHeartbeat,
        named(|r: &mut ConsumerGroupHeartbeatRequest| {
            r.group_id = group("fulfilment");
            r.member_id = s("m");
            r.member_epoch = 5;
        }),
        ConsumerGroupHeartbeatResponse::default()
    );
    case!(
        ConsumerGroupDescribe,
        named(|r: &mut ConsumerGroupDescribeRequest| r.group_ids = vec![group("fulfilment")]),
        ConsumerGroupDescribeResponse::default()
    );
    case!(
        DescribeUserScramCredentials,
        DescribeUserScramCredentialsRequest::default(),
        DescribeUserScramCredentialsResponse::default()
    );
    case!(
        AlterUserScramCredentials,
        named(|r: &mut AlterUserScramCredentialsRequest| {
            r.upsertions = vec![named(
                |u: &mut alter_user_scram_credentials_request::ScramCredentialUpsertion| {
                    u.name = s("alice");
                    u.salt = Bytes::from_static(b"PLANTED-SALT");
                    u.salted_password = Bytes::from_static(b"PLANTED-SALTED-PASSWORD");
                },
            )];
        }),
        AlterUserScramCredentialsResponse::default()
    );
    case!(
        OffsetForLeaderEpoch,
        named(|r: &mut OffsetForLeaderEpochRequest| {
            r.topics = vec![named(
                |t: &mut offset_for_leader_epoch_request::OffsetForLeaderTopic| {
                    t.topic = topic("orders");
                    t.partitions = vec![named(
                        |p: &mut offset_for_leader_epoch_request::OffsetForLeaderPartition| {
                            p.partition = 1;
                            p.leader_epoch = 0;
                        },
                    )];
                },
            )];
        }),
        named(|r: &mut OffsetForLeaderEpochResponse| {
            r.topics = vec![named(
                |t: &mut offset_for_leader_epoch_response::OffsetForLeaderTopicResult| {
                    t.topic = topic("orders");
                    t.partitions = vec![named(
                        |p: &mut offset_for_leader_epoch_response::EpochEndOffset| {
                            p.partition = 1;
                            p.leader_epoch = 0;
                            p.end_offset = 42;
                        },
                    )];
                },
            )];
        })
    );
    case!(
        DescribeLogDirs,
        DescribeLogDirsRequest::default(),
        named(|r: &mut DescribeLogDirsResponse| {
            r.results = vec![named(
                |d: &mut describe_log_dirs_response::DescribeLogDirsResult| {
                    d.log_dir = s("/PLANTED-PATH");
                    d.topics = vec![named(
                        |t: &mut describe_log_dirs_response::DescribeLogDirsTopic| {
                            t.name = topic("orders");
                            t.partitions = vec![
                                describe_log_dirs_response::DescribeLogDirsPartition::default(),
                            ];
                        },
                    )];
                },
            )];
        })
    );
    case!(
        DescribeClientQuotas,
        named(|r: &mut DescribeClientQuotasRequest| {
            r.components = vec![named(
                |c: &mut describe_client_quotas_request::ComponentData| {
                    c.entity_type = s("user");
                    c._match = Some(s("PLANTED-USER"));
                },
            )];
        }),
        DescribeClientQuotasResponse::default()
    );
    case!(
        AlterClientQuotas,
        named(|r: &mut AlterClientQuotasRequest| {
            r.entries = vec![named(|e: &mut alter_client_quotas_request::EntryData| {
                e.entity = vec![named(|p: &mut alter_client_quotas_request::EntityData| {
                    p.entity_type = s("user");
                    p.entity_name = Some(s("PLANTED-USER"));
                })];
            })];
        }),
        named(|r: &mut AlterClientQuotasResponse| {
            r.entries = vec![named(|e: &mut alter_client_quotas_response::EntryData| {
                e.error_code = 42;
                e.error_message = Some(s("Quota producer_byte_rate must be greater than 0"));
            })];
        })
    );
    case!(
        GetTelemetrySubscriptions,
        GetTelemetrySubscriptionsRequest::default(),
        GetTelemetrySubscriptionsResponse::default()
    );
    case!(
        PushTelemetry,
        named(|r: &mut PushTelemetryRequest| r.metrics = Bytes::from_static(b"PLANTED-METRICS")),
        PushTelemetryResponse::default()
    );
}
