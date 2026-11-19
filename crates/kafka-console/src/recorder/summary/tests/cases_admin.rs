//! The admin APIs' cases (debt K3): a page of partitions, an election, reassignments, a log move, config resources, a feature
//! update, and the four token APIs (whose HMACs and ids are planted, and must not be kept).

use bytes::Bytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::*;

use super::cases::{named, s, topic};

pub(super) fn admin(out: &mut Vec<(ApiKey, Request, Response)>) {
    macro_rules! case {
        ($key:ident, $req:expr, $resp:expr) => {
            out.push((
                ApiKey::$key,
                Request::$key($req.into()),
                Response::$key($resp.into()),
            ));
        };
    }
    case!(
        DescribeTopicPartitions,
        named(|r: &mut DescribeTopicPartitionsRequest| {
            r.topics = vec![named(
                |t: &mut describe_topic_partitions_request::TopicRequest| t.name = topic("orders"),
            )];
        }),
        named(|r: &mut DescribeTopicPartitionsResponse| {
            r.topics = vec![named(|t: &mut describe_topic_partitions_response::DescribeTopicPartitionsResponseTopic| {
                t.name = Some(topic("orders"));
            })];
            r.next_cursor = Some(named(
                |c: &mut describe_topic_partitions_response::Cursor| c.topic_name = topic("orders"),
            ));
        })
    );
    case!(
        ElectLeaders,
        named(|r: &mut ElectLeadersRequest| {
            r.topic_partitions = Some(vec![named(
                |t: &mut elect_leaders_request::TopicPartitions| {
                    t.topic = topic("orders");
                    t.partitions = vec![0];
                },
            )]);
        }),
        named(|r: &mut ElectLeadersResponse| {
            r.replica_election_results = vec![named(
                |t: &mut elect_leaders_response::ReplicaElectionResult| {
                    t.topic = topic("orders");
                    t.partition_result =
                        vec![named(|p: &mut elect_leaders_response::PartitionResult| {
                            p.error_code = 84
                        })];
                },
            )];
        })
    );
    case!(
        AlterPartitionReassignments,
        named(|r: &mut AlterPartitionReassignmentsRequest| {
            r.topics = vec![named(
                |t: &mut alter_partition_reassignments_request::ReassignableTopic| {
                    t.name = topic("orders");
                    t.partitions = vec![named(
                        |p: &mut alter_partition_reassignments_request::ReassignablePartition| {
                            p.replicas = Some(vec![BrokerId(2)]);
                        },
                    )];
                },
            )];
        }),
        named(|r: &mut AlterPartitionReassignmentsResponse| {
            r.error_message = None;
            r.responses = vec![named(
                |t: &mut alter_partition_reassignments_response::ReassignableTopicResponse| {
                    t.name = topic("orders");
                    t.partitions = vec![named(|p: &mut alter_partition_reassignments_response::ReassignablePartitionResponse| {
                    p.error_code = 39;
                })];
                },
            )];
        })
    );
    case!(
        ListPartitionReassignments,
        ListPartitionReassignmentsRequest::default(),
        ListPartitionReassignmentsResponse::default()
    );
    case!(
        AlterReplicaLogDirs,
        named(|r: &mut AlterReplicaLogDirsRequest| {
            r.dirs = vec![named(
                |d: &mut alter_replica_log_dirs_request::AlterReplicaLogDir| {
                    d.path = s("/data");
                    d.topics = vec![named(
                        |t: &mut alter_replica_log_dirs_request::AlterReplicaLogDirTopic| {
                            t.name = topic("orders");
                            t.partitions = vec![0];
                        },
                    )];
                },
            )];
        }),
        named(|r: &mut AlterReplicaLogDirsResponse| {
            r.results = vec![named(
                |t: &mut alter_replica_log_dirs_response::AlterReplicaLogDirTopicResult| {
                    t.topic_name = topic("orders");
                    t.partitions = vec![named(|p: &mut alter_replica_log_dirs_response::AlterReplicaLogDirPartitionResult| {
                    p.error_code = 0;
                })];
                },
            )];
        })
    );
    case!(
        ListConfigResources,
        ListConfigResourcesRequest::default(),
        named(|r: &mut ListConfigResourcesResponse| {
            r.config_resources = vec![named(
                |c: &mut list_config_resources_response::ConfigResource| {
                    c.resource_name = s("orders")
                },
            )];
        })
    );
    case!(
        UpdateFeatures,
        named(|r: &mut UpdateFeaturesRequest| {
            r.validate_only = true;
            r.feature_updates = vec![named(
                |u: &mut update_features_request::FeatureUpdateKey| {
                    u.feature = s("transaction.version");
                    u.max_version_level = 1;
                },
            )];
        }),
        named(|r: &mut UpdateFeaturesResponse| {
            r.error_code = 95;
            r.error_message = Some(s("The update failed for all features"));
        })
    );
    let planted = Bytes::from_static(b"PLANTED-HMAC");
    case!(
        CreateDelegationToken,
        CreateDelegationTokenRequest::default(),
        named(|r: &mut CreateDelegationTokenResponse| {
            r.token_id = s("PLANTED-TOKEN-ID");
            r.hmac = planted.clone();
        })
    );
    case!(
        RenewDelegationToken,
        named(|r: &mut RenewDelegationTokenRequest| r.hmac = planted.clone()),
        RenewDelegationTokenResponse::default()
    );
    case!(
        ExpireDelegationToken,
        named(|r: &mut ExpireDelegationTokenRequest| r.hmac = planted.clone()),
        named(|r: &mut ExpireDelegationTokenResponse| r.error_code = 62)
    );
    case!(
        DescribeDelegationToken,
        DescribeDelegationTokenRequest::default(),
        named(|r: &mut DescribeDelegationTokenResponse| {
            r.tokens = vec![named(
                |t: &mut describe_delegation_token_response::DescribedDelegationToken| {
                    t.token_id = s("PLANTED-TOKEN-ID");
                    t.hmac = planted.clone();
                },
            )];
        })
    );
}
