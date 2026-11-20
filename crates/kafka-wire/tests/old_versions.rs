//! The versions Kafka 4.0 removed (KIP-896) that the broker reads again for the older profiles (vendor/kafka-protocol
//! PATCHES.md, patch 5): v0 of the three ACL APIs and of DescribeLogDirs, v0 and v1 of OffsetForLeaderEpoch. Each is
//! held to the bytes Kafka's schema gives it, written by hand here, so a field put back at the wrong version fails.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use bytes::{BufMut, Bytes, BytesMut};
use kafka_protocol::messages::create_acls_request::AclCreation;
use kafka_protocol::messages::delete_acls_response::{
    DeleteAclsFilterResult, DeleteAclsMatchingAcl,
};
use kafka_protocol::messages::describe_acls_response::{AclDescription, DescribeAclsResource};
use kafka_protocol::messages::offset_for_leader_epoch_response::{
    EpochEndOffset, OffsetForLeaderTopicResult,
};
use kafka_protocol::messages::{
    ApiKey, CreateAclsRequest, DeleteAclsRequest, DeleteAclsResponse, DescribeAclsRequest,
    DescribeAclsResponse, DescribeLogDirsRequest, OffsetForLeaderEpochRequest,
    OffsetForLeaderEpochResponse,
};
use kafka_protocol::protocol::{Decodable, Encodable, StrBytes};

fn bytes<T: Encodable>(m: &T, version: i16) -> Bytes {
    let mut out = BytesMut::new();
    m.encode(&mut out, version).unwrap();
    assert_eq!(out.len(), m.compute_size(version).unwrap(), "v{version}");
    out.freeze()
}

fn back<T: Decodable>(mut b: Bytes, version: i16) -> T {
    let m = T::decode(&mut b, version).unwrap();
    assert!(b.is_empty(), "v{version}: bytes left over");
    m
}

fn string(out: &mut BytesMut, s: &str) {
    out.put_i16(i16::try_from(s.len()).unwrap());
    out.put_slice(s.as_bytes());
}

#[test]
fn the_five_keys_start_at_zero() {
    for key in [
        ApiKey::DescribeAcls,
        ApiKey::CreateAcls,
        ApiKey::DeleteAcls,
        ApiKey::OffsetForLeaderEpoch,
        ApiKey::DescribeLogDirs,
    ] {
        assert_eq!(key.valid_versions().min, 0, "{key:?}");
    }
}

#[test]
fn create_acls_v0_has_no_pattern_type_and_reads_as_literal() {
    // CreateAclsRequest v0: [resource_type, resource_name, principal, host, operation, permission].
    let mut wire = BytesMut::new();
    wire.put_i32(1);
    wire.put_i8(2);
    string(&mut wire, "orders");
    string(&mut wire, "User:alice");
    string(&mut wire, "*");
    wire.put_i8(3);
    wire.put_i8(3);
    let read: CreateAclsRequest = back(wire.clone().freeze(), 0);
    let c = &read.creations[0];
    assert_eq!((c.resource_type, c.resource_pattern_type), (2, 3));
    assert_eq!((c.operation, c.permission_type), (3, 3));
    assert_eq!(c.principal.as_str(), "User:alice");
    // Written back at v0 it is the same bytes; v1 has the pattern type byte more.
    assert_eq!(bytes(&read, 0), wire.freeze());
    assert_eq!(bytes(&read, 1).len(), bytes(&read, 0).len() + 1);
    // A PREFIXED creation cannot be written at v0.
    let mut prefixed = AclCreation::default();
    prefixed.resource_pattern_type = 4;
    let req = CreateAclsRequest::default().with_creations(vec![prefixed]);
    assert!(req.encode(&mut BytesMut::new(), 0).is_err());
}

#[test]
fn describe_acls_v0_filters_literal_and_answers_without_pattern_types() {
    // DescribeAclsRequest v0: resource_type, resource_name (nullable), principal, host, operation, permission.
    let mut wire = BytesMut::new();
    wire.put_i8(1);
    wire.put_i16(-1);
    wire.put_i16(-1);
    wire.put_i16(-1);
    wire.put_i8(1);
    wire.put_i8(1);
    let read: DescribeAclsRequest = back(wire.freeze(), 0);
    assert_eq!(
        (read.resource_type_filter, read.pattern_type_filter),
        (1, 3)
    );
    assert!(read.resource_name_filter.is_none());

    let acl = AclDescription::default()
        .with_principal(StrBytes::from_static_str("User:alice"))
        .with_host(StrBytes::from_static_str("*"))
        .with_operation(3)
        .with_permission_type(3);
    let resource = DescribeAclsResource::default()
        .with_resource_type(2)
        .with_resource_name(StrBytes::from_static_str("orders"))
        .with_pattern_type(3)
        .with_acls(vec![acl]);
    let resp = DescribeAclsResponse::default().with_resources(vec![resource.clone()]);
    // throttle, error, message (the default is empty), [type, name, [principal, host, op, perm]].
    let mut want = BytesMut::new();
    want.put_i32(0);
    want.put_i16(0);
    want.put_i16(0);
    want.put_i32(1);
    want.put_i8(2);
    string(&mut want, "orders");
    want.put_i32(1);
    string(&mut want, "User:alice");
    string(&mut want, "*");
    want.put_i8(3);
    want.put_i8(3);
    assert_eq!(bytes(&resp, 0), want.freeze());
    let read: DescribeAclsResponse = back(bytes(&resp, 0), 0);
    assert_eq!(read.resources[0].pattern_type, 3);
    // A PREFIXED resource cannot be described at v0.
    let prefixed =
        DescribeAclsResponse::default().with_resources(vec![resource.with_pattern_type(4)]);
    assert!(prefixed.encode(&mut BytesMut::new(), 0).is_err());
}

#[test]
fn delete_acls_v0_filters_literal_and_answers_without_pattern_types() {
    let mut wire = BytesMut::new();
    wire.put_i32(1);
    wire.put_i8(2);
    string(&mut wire, "orders");
    wire.put_i16(-1);
    wire.put_i16(-1);
    wire.put_i8(1);
    wire.put_i8(1);
    let read: DeleteAclsRequest = back(wire.freeze(), 0);
    assert_eq!(read.filters[0].pattern_type_filter, 3);
    assert_eq!(
        read.filters[0]
            .resource_name_filter
            .as_ref()
            .unwrap()
            .as_str(),
        "orders"
    );

    let matching = DeleteAclsMatchingAcl::default()
        .with_resource_type(2)
        .with_resource_name(StrBytes::from_static_str("orders"))
        .with_pattern_type(3)
        .with_principal(StrBytes::from_static_str("User:alice"))
        .with_host(StrBytes::from_static_str("*"))
        .with_operation(3)
        .with_permission_type(3);
    let resp = DeleteAclsResponse::default().with_filter_results(vec![
        DeleteAclsFilterResult::default().with_matching_acls(vec![matching]),
    ]);
    // The v1 answer has the pattern type byte more, and v0 reads back as LITERAL.
    assert_eq!(bytes(&resp, 1).len(), bytes(&resp, 0).len() + 1);
    let read: DeleteAclsResponse = back(bytes(&resp, 0), 0);
    assert_eq!(read.filter_results[0].matching_acls[0].pattern_type, 3);
}

#[test]
fn offset_for_leader_epoch_v0_and_v1_have_no_current_epoch_and_v0_answers_no_epoch() {
    // Request v0 and v1: [topic, [partition, leader_epoch]]; v2 adds the current leader epoch.
    let mut wire = BytesMut::new();
    wire.put_i32(1);
    string(&mut wire, "orders");
    wire.put_i32(1);
    wire.put_i32(0);
    wire.put_i32(7);
    for version in [0, 1] {
        let read: OffsetForLeaderEpochRequest = back(wire.clone().freeze(), version);
        let p = &read.topics[0].partitions[0];
        assert_eq!(
            (p.partition, p.leader_epoch, p.current_leader_epoch),
            (0, 7, -1)
        );
        assert_eq!(read.replica_id, -2);
    }

    let resp = OffsetForLeaderEpochResponse::default()
        .with_throttle_time_ms(5)
        .with_topics(vec![
            OffsetForLeaderTopicResult::default()
                .with_topic(StrBytes::from_static_str("orders").into())
                .with_partitions(vec![
                    EpochEndOffset::default()
                        .with_leader_epoch(0)
                        .with_end_offset(42),
                ]),
        ]);
    // v0: [topic, [error, partition, end_offset]]; v1 adds the leader epoch; v2 the throttle time in front.
    let mut want = BytesMut::new();
    want.put_i32(1);
    string(&mut want, "orders");
    want.put_i32(1);
    want.put_i16(0);
    want.put_i32(0);
    want.put_i64(42);
    assert_eq!(bytes(&resp, 0), want.freeze());
    assert_eq!(bytes(&resp, 1).len(), bytes(&resp, 0).len() + 4);
    assert_eq!(bytes(&resp, 2).len(), bytes(&resp, 1).len() + 4);
    let read: OffsetForLeaderEpochResponse = back(bytes(&resp, 0), 0);
    assert_eq!(read.topics[0].partitions[0].leader_epoch, -1);
}

#[test]
fn describe_log_dirs_v0_is_v1() {
    let req = DescribeLogDirsRequest::default();
    assert_eq!(bytes(&req, 0), bytes(&req, 1));
    let _: DescribeLogDirsRequest = back(bytes(&req, 1), 0);
}

/// Patch 7: v0 of AlterReplicaLogDirs and of the four delegation-token APIs is v1 on the wire ("Version 1 is the same as version
/// 0" in each 3.9.1 schema), so each message reads and writes the same bytes at both, and v0 is decodable. A RenewDelegationToken
/// request is also held to its bytes written by hand: the HMAC (int32 length, then the bytes), then the period (int64).
#[test]
fn patch_7_v0_of_the_log_dirs_and_token_apis_is_v1() {
    use kafka_protocol::messages::alter_replica_log_dirs_request::{
        AlterReplicaLogDir, AlterReplicaLogDirTopic,
    };
    use kafka_protocol::messages::{
        AlterReplicaLogDirsRequest, AlterReplicaLogDirsResponse, CreateDelegationTokenRequest,
        CreateDelegationTokenResponse, DescribeDelegationTokenRequest,
        DescribeDelegationTokenResponse, ExpireDelegationTokenRequest,
        ExpireDelegationTokenResponse, RenewDelegationTokenRequest, RenewDelegationTokenResponse,
        TopicName,
    };
    for key in [
        ApiKey::AlterReplicaLogDirs,
        ApiKey::CreateDelegationToken,
        ApiKey::RenewDelegationToken,
        ApiKey::ExpireDelegationToken,
        ApiKey::DescribeDelegationToken,
    ] {
        assert_eq!(key.valid_versions().min, 0, "{key:?}");
    }
    fn same<T: Encodable + Decodable + PartialEq + std::fmt::Debug>(m: &T) {
        let (v0, v1) = (bytes(m, 0), bytes(m, 1));
        assert_eq!(v0, v1);
        assert_eq!(&back::<T>(v0, 0), m);
    }
    let mut dirs = AlterReplicaLogDirsRequest::default();
    let mut d = AlterReplicaLogDir::default();
    d.path = StrBytes::from_static_str("/data");
    let mut t = AlterReplicaLogDirTopic::default();
    t.name = TopicName(StrBytes::from_static_str("t"));
    t.partitions = vec![0, 2];
    d.topics.push(t);
    dirs.dirs.push(d);
    same(&dirs);
    same(&AlterReplicaLogDirsResponse::default());
    same(&CreateDelegationTokenRequest::default());
    same(&CreateDelegationTokenResponse::default());
    same(&DescribeDelegationTokenRequest::default());
    same(&DescribeDelegationTokenResponse::default());
    same(&ExpireDelegationTokenResponse::default());
    same(&RenewDelegationTokenResponse::default());
    let mut renew = RenewDelegationTokenRequest::default();
    renew.hmac = Bytes::from_static(&[1, 2, 3]);
    renew.renew_period_ms = 60_000;
    same(&renew);
    let mut expect = BytesMut::new();
    expect.put_i32(3);
    expect.put_slice(&[1, 2, 3]);
    expect.put_i64(60_000);
    assert_eq!(bytes(&renew, 0), expect.freeze());
    let mut expire = ExpireDelegationTokenRequest::default();
    expire.hmac = Bytes::from_static(&[9]);
    expire.expiry_time_period_ms = -1;
    same(&expire);
}
