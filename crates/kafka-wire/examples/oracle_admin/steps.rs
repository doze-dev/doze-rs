//! The steps of every admin API but the token ones, and who is refused each.

use kafka_protocol::messages::alter_partition_reassignments_request::{
    ReassignablePartition, ReassignableTopic,
};
use kafka_protocol::messages::alter_replica_log_dirs_request::{
    AlterReplicaLogDir, AlterReplicaLogDirTopic,
};
use kafka_protocol::messages::create_topics_request::CreatableTopic;
use kafka_protocol::messages::describe_topic_partitions_request::{Cursor, TopicRequest};
use kafka_protocol::messages::elect_leaders_request::TopicPartitions;
use kafka_protocol::messages::incremental_alter_configs_request::{
    AlterConfigsResource, AlterableConfig,
};
use kafka_protocol::messages::list_partition_reassignments_request::ListPartitionReassignmentsTopics;
use kafka_protocol::messages::{
    AlterPartitionReassignmentsRequest, AlterPartitionReassignmentsResponse,
    AlterReplicaLogDirsRequest, AlterReplicaLogDirsResponse, ApiKey, BrokerId, CreateTopicsRequest,
    CreateTopicsResponse, DescribeLogDirsRequest, DescribeLogDirsResponse,
    DescribeTopicPartitionsRequest, DescribeTopicPartitionsResponse, ElectLeadersRequest,
    ElectLeadersResponse, IncrementalAlterConfigsRequest, IncrementalAlterConfigsResponse,
    ListConfigResourcesRequest, ListConfigResourcesResponse, ListPartitionReassignmentsRequest,
    ListPartitionReassignmentsResponse, TopicName,
};
use kafka_protocol::protocol::StrBytes;

use crate::conn::{Pair, Tally};

pub(crate) fn s(text: &str) -> StrBytes {
    StrBytes::from_string(text.to_owned())
}

pub(crate) fn topic(name: &str) -> TopicName {
    TopicName(s(name))
}

pub(crate) fn internal(name: &str) -> bool {
    name.starts_with("__")
}

/// Kafka's CLUSTER_AUTHORIZATION_FAILED text names its own request object; what is compared is the operation it needs.
pub(crate) fn denied(message: &mut Option<StrBytes>) {
    let op = (message.as_ref())
        .filter(|m| m.starts_with("Request "))
        .and_then(|m| m.rsplit_once(" needs ").map(|(_, op)| op.to_owned()));
    if let Some(op) = op {
        *message = Some(s(&format!("Request <request> needs {op}")));
    }
}

pub(crate) fn settle() {
    std::thread::sleep(std::time::Duration::from_secs(2));
}

/// Three topics on both, and a config on a group (for ListConfigResources).
pub(crate) fn fixture(p: &mut Pair) {
    let mut req = CreateTopicsRequest::default();
    req.timeout_ms = 30_000;
    for (name, partitions) in [("oa-one", 1), ("oa-three", 3), ("oa-many", 5)] {
        let mut t = CreatableTopic::default();
        t.name = topic(name);
        t.num_partitions = partitions;
        t.replication_factor = 1;
        req.topics.push(t);
    }
    let _: (CreateTopicsResponse, CreateTopicsResponse) = p.both(ApiKey::CreateTopics, 7, &req);
    let mut alter = IncrementalAlterConfigsRequest::default();
    let mut r = AlterConfigsResource::default();
    r.resource_type = 32;
    r.resource_name = s("oa-group");
    let mut c = AlterableConfig::default();
    c.name = s("share.record.lock.duration.ms");
    c.value = Some(s("20000"));
    r.configs.push(c);
    alter.resources.push(r);
    let (k, d): (
        IncrementalAlterConfigsResponse,
        IncrementalAlterConfigsResponse,
    ) = p.both(ApiKey::IncrementalAlterConfigs, 1, &alter);
    assert_eq!(
        (k.responses[0].error_code, d.responses[0].error_code),
        (0, 0),
        "the group config"
    );
    settle();
}

pub(crate) fn dtp(
    names: &[&str],
    limit: i32,
    cursor: Option<(&str, i32)>,
) -> DescribeTopicPartitionsRequest {
    let mut req = DescribeTopicPartitionsRequest::default();
    req.response_partition_limit = limit;
    req.topics = (names.iter())
        .map(|n| {
            let mut t = TopicRequest::default();
            t.name = topic(n);
            t
        })
        .collect();
    req.cursor = cursor.map(|(name, p)| {
        let mut c = Cursor::default();
        c.topic_name = topic(name);
        c.partition_index = p;
        c
    });
    req
}

pub(crate) fn norm_dtp(r: &mut DescribeTopicPartitionsResponse) {
    r.topics
        .retain(|t| !t.name.as_ref().is_some_and(|n| internal(&n.0)));
    for t in &mut r.topics {
        if !t.topic_id.is_nil() {
            t.topic_id = uuid_one();
        }
    }
    // The refused come last, from a hash set in Kafka.
    let (mut refused, rest): (Vec<_>, Vec<_>) =
        r.topics.drain(..).partition(|t| t.error_code == 29);
    refused.sort_by(|a, b| {
        a.name
            .as_ref()
            .map(|n| n.0.to_string())
            .cmp(&b.name.as_ref().map(|n| n.0.to_string()))
    });
    r.topics = rest;
    r.topics.extend(refused);
}

pub(crate) fn uuid_one() -> kafka_wire::Uuid {
    kafka_wire::Uuid::from_u128(1)
}

pub(crate) fn describe_topic_partitions(p: &mut Pair, t: &mut Tally) {
    let k = ApiKey::DescribeTopicPartitions;
    let cases: &[(&str, DescribeTopicPartitionsRequest)] = &[
        ("all topics", dtp(&[], 2000, None)),
        (
            "named, missing and invalid",
            dtp(
                &["oa-three", "oa-one", "nothing", "bad name!", "oa-one"],
                2000,
                None,
            ),
        ),
        ("a page of two", dtp(&[], 2, None)),
        (
            "a page of four from a cursor",
            dtp(&[], 4, Some(("oa-many", 3))),
        ),
        (
            "named from a cursor",
            dtp(&["oa-three", "oa-many"], 3, Some(("oa-three", 1))),
        ),
        ("a limit of zero is one", dtp(&["oa-three"], 0, None)),
        (
            "a cursor past the partitions",
            dtp(&["oa-three"], 10, Some(("oa-three", 7))),
        ),
        (
            "a cursor topic not named",
            dtp(&["oa-one"], 10, Some(("oa-three", 0))),
        ),
        (
            "a negative cursor partition",
            dtp(&["oa-three"], 10, Some(("oa-three", -1))),
        ),
        (
            "a full page before a missing topic",
            dtp(&["oa-one", "oa-zzz"], 1, None),
        ),
    ];
    for (name, req) in cases {
        p.same(t, name, k, 0, req, norm_dtp);
    }
}

pub(crate) fn elect(kind: i8, asks: Option<&[(&str, &[i32])]>) -> ElectLeadersRequest {
    let mut req = ElectLeadersRequest::default();
    req.election_type = kind;
    req.timeout_ms = 30_000;
    req.topic_partitions = asks.map(|asks| {
        asks.iter()
            .map(|(name, parts)| {
                let mut tp = TopicPartitions::default();
                tp.topic = topic(name);
                tp.partitions = parts.to_vec();
                tp
            })
            .collect()
    });
    req
}

pub(crate) fn norm_elect(r: &mut ElectLeadersResponse) {
    r.replica_election_results.retain(|t| !internal(&t.topic.0));
    r.replica_election_results
        .sort_by(|a, b| a.topic.0.cmp(&b.topic.0));
    for t in &mut r.replica_election_results {
        for p in &mut t.partition_result {
            denied(&mut p.error_message);
        }
    }
}

pub(crate) fn elect_leaders(p: &mut Pair, t: &mut Tally) {
    let k = ApiKey::ElectLeaders;
    let asks: &[(&str, &[i32])] = &[
        ("oa-three", &[0, 2, 9]),
        ("nothing", &[0]),
        ("oa-one", &[-1]),
    ];
    for v in 0..=2 {
        p.same(
            t,
            "preferred, named",
            k,
            v,
            &elect(0, Some(asks)),
            norm_elect,
        );
        p.same(t, "every partition", k, v, &elect(0, None), norm_elect);
        p.same(t, "an empty list", k, v, &elect(0, Some(&[])), norm_elect);
    }
    for v in 1..=2 {
        p.same(t, "unclean, named", k, v, &elect(1, Some(asks)), norm_elect);
        p.same(
            t,
            "an unknown election type",
            k,
            v,
            &elect(5, Some(asks)),
            norm_elect,
        );
        p.same(
            t,
            "an unknown type, every partition",
            k,
            v,
            &elect(5, None),
            norm_elect,
        );
    }
}

pub(crate) fn reassign(
    asks: &[(&str, i32, Option<&[i32]>)],
    allow_rf: bool,
) -> AlterPartitionReassignmentsRequest {
    let mut req = AlterPartitionReassignmentsRequest::default();
    req.timeout_ms = 30_000;
    req.allow_replication_factor_change = allow_rf;
    for (name, part, replicas) in asks {
        let mut tp = ReassignableTopic::default();
        tp.name = topic(name);
        let mut rp = ReassignablePartition::default();
        rp.partition_index = *part;
        rp.replicas = replicas.map(|r| r.iter().map(|&b| BrokerId(b)).collect());
        tp.partitions.push(rp);
        req.topics.push(tp);
    }
    req
}

pub(crate) fn norm_reassign(r: &mut AlterPartitionReassignmentsResponse) {
    denied(&mut r.error_message);
    for t in &mut r.responses {
        for p in &mut t.partitions {
            denied(&mut p.error_message);
        }
    }
}

pub(crate) fn list_reassign(asks: Option<&[(&str, &[i32])]>) -> ListPartitionReassignmentsRequest {
    let mut req = ListPartitionReassignmentsRequest::default();
    req.timeout_ms = 30_000;
    req.topics = asks.map(|asks| {
        asks.iter()
            .map(|(name, parts)| {
                let mut tp = ListPartitionReassignmentsTopics::default();
                tp.name = topic(name);
                tp.partition_indexes = parts.to_vec();
                tp
            })
            .collect()
    });
    req
}

pub(crate) fn norm_list(r: &mut ListPartitionReassignmentsResponse) {
    denied(&mut r.error_message);
}

pub(crate) fn reassignments(p: &mut Pair, t: &mut Tally) {
    let k = ApiKey::AlterPartitionReassignments;
    let asks: &[(&str, i32, Option<&[i32]>)] = &[
        ("oa-three", 0, Some(&[1])),
        ("oa-three", 1, None),
        ("oa-three", 2, Some(&[])),
        ("oa-many", 0, Some(&[2])),
        ("oa-many", 1, Some(&[1, 1])),
        ("oa-many", 2, Some(&[2, 1])),
        ("oa-many", 7, Some(&[1])),
        ("nothing", 0, Some(&[1])),
    ];
    for v in 0..=1 {
        p.same(
            t,
            "each refusal and a move to where it is",
            k,
            v,
            &reassign(asks, true),
            norm_reassign,
        );
        p.same(
            t,
            "nothing asked",
            k,
            v,
            &reassign(&[], true),
            norm_reassign,
        );
    }
    p.same(
        t,
        "the factor kept",
        k,
        1,
        &reassign(
            &[("oa-one", 0, Some(&[1])), ("oa-one", 0, Some(&[1, 2]))],
            false,
        ),
        norm_reassign,
    );
    let k = ApiKey::ListPartitionReassignments;
    p.same(
        t,
        "every reassignment",
        k,
        0,
        &list_reassign(None),
        norm_list,
    );
    p.same(
        t,
        "named",
        k,
        0,
        &list_reassign(Some(&[("oa-three", &[0, 1, 9]), ("nothing", &[0])])),
        norm_list,
    );
    p.same(
        t,
        "an empty list",
        k,
        0,
        &list_reassign(Some(&[])),
        norm_list,
    );
}

/// Each broker's own log directory, as DescribeLogDirs says it.
pub(crate) fn dirs(p: &mut Pair) -> (String, String) {
    let (k, d): (DescribeLogDirsResponse, DescribeLogDirsResponse) = p.both(
        ApiKey::DescribeLogDirs,
        4,
        &DescribeLogDirsRequest::default(),
    );
    (
        k.results[0].log_dir.to_string(),
        d.results[0].log_dir.to_string(),
    )
}

pub(crate) fn move_logs(asks: &[(&str, &str, &[i32])]) -> AlterReplicaLogDirsRequest {
    let mut req = AlterReplicaLogDirsRequest::default();
    for (dir, name, parts) in asks {
        let mut d = AlterReplicaLogDir::default();
        d.path = s(dir);
        let mut tp = AlterReplicaLogDirTopic::default();
        tp.name = topic(name);
        tp.partitions = parts.to_vec();
        d.topics.push(tp);
        req.dirs.push(d);
    }
    req
}

pub(crate) fn norm_move(r: &mut AlterReplicaLogDirsResponse) {
    r.results
        .sort_by(|a, b| a.topic_name.0.cmp(&b.topic_name.0));
    for t in &mut r.results {
        t.partitions.sort_by_key(|p| p.partition_index);
    }
}

pub(crate) fn log_dirs(p: &mut Pair, t: &mut Tally) {
    let (kd, dd) = dirs(p);
    let long = "l".repeat(220);
    let req = |dir: &str| {
        move_logs(&[
            (dir, "oa-three", &[0, 1, 9]),
            ("/no/such/dir", "oa-one", &[0]),
            (dir, "nothing", &[0]),
            (dir, long.as_str(), &[0]),
            (dir, "oa-many", &[4]),
        ])
    };
    for v in 1..=2 {
        p.same2(
            t,
            "to its own directory, another and none",
            ApiKey::AlterReplicaLogDirs,
            v,
            (&req(&kd), &req(&dd)),
            norm_move,
            None,
        );
    }
}

pub(crate) fn config_resources(types: &[i8]) -> ListConfigResourcesRequest {
    let mut req = ListConfigResourcesRequest::default();
    req.resource_types = types.to_vec();
    req
}

pub(crate) fn norm_resources(r: &mut ListConfigResourcesResponse) {
    r.config_resources
        .retain(|c| !(c.resource_type == 2 && internal(&c.resource_name)));
    r.config_resources.sort_by(|a, b| {
        (a.resource_type, a.resource_name.to_string())
            .cmp(&(b.resource_type, b.resource_name.to_string()))
    });
}
