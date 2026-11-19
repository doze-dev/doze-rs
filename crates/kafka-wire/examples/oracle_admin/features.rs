//! ListConfigResources, UpdateFeatures and the features ApiVersions lists, and bob's refusals on every admin API.

use kafka_protocol::messages::create_acls_request::AclCreation;
use kafka_protocol::messages::update_features_request::FeatureUpdateKey;
use kafka_protocol::messages::{
    ApiKey, ApiVersionsRequest, ApiVersionsResponse, CreateAclsRequest, CreateAclsResponse,
    UpdateFeaturesRequest, UpdateFeaturesResponse,
};

use crate::conn::{Pair, Tally};
use crate::steps::{
    config_resources, denied, dirs, dtp, elect, list_reassign, move_logs, norm_dtp, norm_elect,
    norm_list, norm_move, norm_reassign, norm_resources, reassign, s, settle,
};

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
