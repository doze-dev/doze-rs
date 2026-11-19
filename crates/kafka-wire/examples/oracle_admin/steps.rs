//! The steps of every admin API but the token ones, and who is refused each.

use kafka_protocol::messages::alter_partition_reassignments_request::{
    ReassignablePartition, ReassignableTopic,
};
use kafka_protocol::messages::alter_replica_log_dirs_request::{
    AlterReplicaLogDir, AlterReplicaLogDirTopic,
};
use kafka_protocol::messages::create_acls_request::AclCreation;
use kafka_protocol::messages::create_topics_request::CreatableTopic;
use kafka_protocol::messages::describe_topic_partitions_request::{Cursor, TopicRequest};
use kafka_protocol::messages::elect_leaders_request::TopicPartitions;
use kafka_protocol::messages::incremental_alter_configs_request::{
    AlterConfigsResource, AlterableConfig,
};
use kafka_protocol::messages::list_partition_reassignments_request::ListPartitionReassignmentsTopics;
use kafka_protocol::messages::update_features_request::FeatureUpdateKey;
use kafka_protocol::messages::{
    AlterPartitionReassignmentsRequest, AlterPartitionReassignmentsResponse,
    AlterReplicaLogDirsRequest, AlterReplicaLogDirsResponse, ApiKey, ApiVersionsRequest,
    ApiVersionsResponse, BrokerId, CreateAclsRequest, CreateAclsResponse, CreateTopicsRequest,
    CreateTopicsResponse, DescribeLogDirsRequest, DescribeLogDirsResponse,
    DescribeTopicPartitionsRequest, DescribeTopicPartitionsResponse, ElectLeadersRequest,
    ElectLeadersResponse, IncrementalAlterConfigsRequest, IncrementalAlterConfigsResponse,
    ListConfigResourcesRequest, ListConfigResourcesResponse, ListPartitionReassignmentsRequest,
    ListPartitionReassignmentsResponse, TopicName, UpdateFeaturesRequest, UpdateFeaturesResponse,
};
use kafka_protocol::protocol::StrBytes;

use crate::conn::{Pair, Tally};

pub(crate) fn s(text: &str) -> StrBytes {
    StrBytes::from_string(text.to_owned())
}

fn topic(name: &str) -> TopicName {
    TopicName(s(name))
}

fn internal(name: &str) -> bool {
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

fn settle() {
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

fn dtp(names: &[&str], limit: i32, cursor: Option<(&str, i32)>) -> DescribeTopicPartitionsRequest {
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

fn norm_dtp(r: &mut DescribeTopicPartitionsResponse) {
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

fn uuid_one() -> kafka_wire::Uuid {
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

fn elect(kind: i8, asks: Option<&[(&str, &[i32])]>) -> ElectLeadersRequest {
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

fn norm_elect(r: &mut ElectLeadersResponse) {
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

fn reassign(
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

fn norm_reassign(r: &mut AlterPartitionReassignmentsResponse) {
    denied(&mut r.error_message);
    for t in &mut r.responses {
        for p in &mut t.partitions {
            denied(&mut p.error_message);
        }
    }
}

fn list_reassign(asks: Option<&[(&str, &[i32])]>) -> ListPartitionReassignmentsRequest {
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

fn norm_list(r: &mut ListPartitionReassignmentsResponse) {
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

fn move_logs(asks: &[(&str, &str, &[i32])]) -> AlterReplicaLogDirsRequest {
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

fn norm_move(r: &mut AlterReplicaLogDirsResponse) {
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

fn config_resources(types: &[i8]) -> ListConfigResourcesRequest {
    let mut req = ListConfigResourcesRequest::default();
    req.resource_types = types.to_vec();
    req
}

fn norm_resources(r: &mut ListConfigResourcesResponse) {
    r.config_resources
        .retain(|c| !(c.resource_type == 2 && internal(&c.resource_name)));
    r.config_resources.sort_by(|a, b| {
        (a.resource_type, a.resource_name.to_string())
            .cmp(&(b.resource_type, b.resource_name.to_string()))
    });
}

pub(crate) fn list_config_resources(p: &mut Pair, t: &mut Tally) {
    let k = ApiKey::ListConfigResources;
    p.same(
        t,
        "client metrics (v0)",
        k,
        0,
        &config_resources(&[]),
        norm_resources,
    );
    for types in [&[][..], &[2], &[4, 8], &[32], &[16], &[2, 99]] {
        p.same(
            t,
            &format!("types {types:?}"),
            k,
            1,
            &config_resources(types),
            norm_resources,
        );
    }
}

fn update(asks: &[(&str, i16, i8)], validate_only: bool) -> UpdateFeaturesRequest {
    let mut req = UpdateFeaturesRequest::default();
    req.timeout_ms = 30_000;
    req.validate_only = validate_only;
    for (name, level, kind) in asks {
        let mut u = FeatureUpdateKey::default();
        u.feature = s(name);
        u.max_version_level = *level;
        u.upgrade_type = *kind;
        req.feature_updates.push(u);
    }
    req
}

/// The same at `version`: v0 says a downgrade with `allow_downgrade`, later versions with the upgrade type.
fn update_at(version: i16, asks: &[(&str, i16, i8)], validate_only: bool) -> UpdateFeaturesRequest {
    let mut req = update(asks, validate_only);
    if version == 0 {
        // v0 has no validate-only either.
        req.validate_only = false;
        for u in &mut req.feature_updates {
            u.allow_downgrade = u.upgrade_type != 1;
            u.upgrade_type = 1;
        }
    }
    req
}

fn norm_features(r: &mut UpdateFeaturesResponse) {
    denied(&mut r.error_message);
}

pub(crate) fn features(p: &mut Pair, t: &mut Tally) {
    let k = ApiKey::UpdateFeatures;
    for v in 0..=2 {
        let validate = v >= 1;
        p.same(
            t,
            "a safe downgrade, validated",
            k,
            v,
            &update_at(v, &[("transaction.version", 0, 2)], true),
            norm_features,
        );
        p.same(
            t,
            "levels as they are",
            k,
            v,
            &update_at(
                v,
                &[("group.version", 1, 1), ("share.version", 2, 1)],
                false,
            ),
            norm_features,
        );
        p.same(t, "nothing", k, v, &update_at(v, &[], false), norm_features);
        p.same(
            t,
            "a downgrade as an upgrade",
            k,
            v,
            &update_at(v, &[("transaction.version", 1, 1)], validate),
            norm_features,
        );
        p.same(
            t,
            "an upgrade past what is supported",
            k,
            v,
            &update_at(v, &[("transaction.version", 3, 1)], validate),
            norm_features,
        );
        p.same(
            t,
            "an unknown feature",
            k,
            v,
            &update_at(v, &[("no.such.feature", 1, 1)], validate),
            norm_features,
        );
        p.same(
            t,
            "an unknown feature at 0",
            k,
            v,
            &update_at(v, &[("no.such.feature", 0, 1)], validate),
            norm_features,
        );
        p.same(
            t,
            "a negative level",
            k,
            v,
            &update_at(v, &[("group.version", -1, 2)], validate),
            norm_features,
        );
    }
    for v in 1..=2 {
        p.same(
            t,
            "an unknown upgrade type",
            k,
            v,
            &update_at(v, &[("group.version", 0, 9)], true),
            norm_features,
        );
        p.same(
            t,
            "an upgrade asked as a downgrade",
            k,
            v,
            &update_at(v, &[("kraft.version", 1, 2)], true),
            norm_features,
        );
        p.same(
            t,
            "metadata.version down, safe",
            k,
            v,
            &update_at(v, &[("metadata.version", 30, 2)], true),
            norm_features,
        );
        p.same(
            t,
            "metadata.version down, unsafe",
            k,
            v,
            &update_at(v, &[("metadata.version", 30, 3)], true),
            norm_features,
        );
        p.same(
            t,
            "metadata.version past the latest",
            k,
            v,
            &update_at(v, &[("metadata.version", 34, 1)], true),
            norm_features,
        );
        p.same(
            t,
            "kraft.version down",
            k,
            v,
            &update_at(v, &[("kraft.version", 0, 2)], true),
            norm_features,
        );
        p.same(
            t,
            "ELR without its metadata.version",
            k,
            v,
            &update_at(
                v,
                &[
                    ("eligible.leader.replicas.version", 1, 1),
                    ("metadata.version", 22, 2),
                ],
                true,
            ),
            norm_features,
        );
        p.same(
            t,
            "a first error stops the rest",
            k,
            v,
            &update_at(
                v,
                &[("group.version", 0, 2), ("no.such.feature", 2, 1)],
                true,
            ),
            norm_features,
        );
    }
    // A real change: Kafka makes it, this broker keeps its levels (ADM-D9). Kafka is put back after.
    p.same2(
        t,
        "transaction.version down to 1",
        k,
        2,
        (
            &update(&[("transaction.version", 1, 2)], false),
            &update(&[("transaction.version", 1, 2)], false),
        ),
        norm_features,
        Some("ADM-D9: the feature levels do not move"),
    );
    let back = update(&[("transaction.version", 2, 1)], false);
    let _: UpdateFeaturesResponse = p.kafka.call(k, 2, &back);
    settle();
    let mut v = ApiVersionsRequest::default();
    v.client_software_name = s("oracle");
    v.client_software_version = s("1");
    p.same(
        t,
        "the features ApiVersions lists",
        ApiKey::ApiVersions,
        4,
        &v,
        |r: &mut ApiVersionsResponse| {
            r.api_keys.clear();
            r.finalized_features_epoch = r.finalized_features_epoch.min(0);
            r.supported_features.sort_by(|a, b| a.name.cmp(&b.name));
            r.finalized_features.sort_by(|a, b| a.name.cmp(&b.name));
        },
    );
}

fn acl(resource: i8, name: &str, principal: &str, op: i8) -> AclCreation {
    let mut a = AclCreation::default();
    a.resource_type = resource;
    a.resource_name = s(name);
    a.resource_pattern_type = 3;
    a.principal = s(principal);
    a.host = s("*");
    a.operation = op;
    a.permission_type = 3;
    a
}

/// ACLs that name alice on the cluster and on one topic, so that bob, whom none names, is refused; then bob's every request.
pub(crate) fn refused(admin: &mut Pair, bob: &mut Pair, t: &mut Tally) {
    let mut req = CreateAclsRequest::default();
    for op in [7, 8, 10] {
        req.creations
            .push(acl(4, "kafka-cluster", "User:alice", op));
    }
    req.creations.push(acl(2, "oa-three", "User:alice", 8));
    let (k, d): (CreateAclsResponse, CreateAclsResponse) = admin.both(ApiKey::CreateAcls, 3, &req);
    assert!(
        k.results
            .iter()
            .chain(&d.results)
            .all(|r| r.error_code == 0),
        "the ACLs"
    );
    settle();
    let asks: &[(&str, &[i32])] = &[("oa-three", &[0])];
    bob.same(
        t,
        "bob: ElectLeaders",
        ApiKey::ElectLeaders,
        2,
        &elect(0, Some(asks)),
        norm_elect,
    );
    bob.same(
        t,
        "bob: ElectLeaders v0",
        ApiKey::ElectLeaders,
        0,
        &elect(0, Some(asks)),
        norm_elect,
    );
    bob.same(
        t,
        "bob: ElectLeaders of everything",
        ApiKey::ElectLeaders,
        2,
        &elect(0, None),
        norm_elect,
    );
    bob.same(
        t,
        "bob: AlterPartitionReassignments",
        ApiKey::AlterPartitionReassignments,
        1,
        &reassign(&[("oa-three", 0, Some(&[1]))], true),
        norm_reassign,
    );
    bob.same(
        t,
        "bob: ListPartitionReassignments",
        ApiKey::ListPartitionReassignments,
        0,
        &list_reassign(Some(&[("oa-three", &[0, 1])])),
        norm_list,
    );
    bob.same(
        t,
        "bob: ListPartitionReassignments of all",
        ApiKey::ListPartitionReassignments,
        0,
        &list_reassign(None),
        norm_list,
    );
    let (kd, dd) = dirs(admin);
    let mv = |dir: &str| move_logs(&[(dir, "oa-three", &[0, 1]), (dir, "oa-three", &[2])]);
    bob.same2(
        t,
        "bob: AlterReplicaLogDirs",
        ApiKey::AlterReplicaLogDirs,
        2,
        (&mv(&kd), &mv(&dd)),
        norm_move,
        None,
    );
    bob.same(
        t,
        "bob: ListConfigResources",
        ApiKey::ListConfigResources,
        1,
        &config_resources(&[]),
        norm_resources,
    );
    bob.same(
        t,
        "bob: UpdateFeatures",
        ApiKey::UpdateFeatures,
        1,
        &update(&[("group.version", 1, 1)], true),
        norm_features,
    );
    bob.same(
        t,
        "bob: DescribeTopicPartitions, named",
        ApiKey::DescribeTopicPartitions,
        0,
        &dtp(&["oa-one", "oa-three", "nothing"], 2000, None),
        norm_dtp,
    );
    bob.same(
        t,
        "bob: DescribeTopicPartitions, all",
        ApiKey::DescribeTopicPartitions,
        0,
        &dtp(&[], 2000, None),
        norm_dtp,
    );
}
