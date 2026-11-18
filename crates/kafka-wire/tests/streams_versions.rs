//! The streams-group messages (KIP-1071) at the versions Apache Kafka 4.4.0 has (vendor/kafka-protocol/PATCHES.md,
//! patch 4): each field that differs between versions is written and read back where it is, and is not on the wire where
//! it is not.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use bytes::{Bytes, BytesMut};
use kafka_protocol::messages::streams_group_heartbeat_request::{Endpoint, TaskIds, Topology};
use kafka_protocol::messages::{
    ApiKey, StreamsGroupDescribeRequest, StreamsGroupDescribeResponse,
    StreamsGroupHeartbeatRequest, StreamsGroupHeartbeatResponse,
    StreamsGroupTopologyDescriptionUpdateResponse,
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

#[test]
fn the_three_keys_are_at_kafka_4_4_versions() {
    let range = |k: ApiKey| (k.valid_versions().min, k.valid_versions().max);
    assert_eq!(range(ApiKey::StreamsGroupHeartbeat), (0, 1));
    assert_eq!(range(ApiKey::StreamsGroupDescribe), (0, 1));
    assert_eq!(range(ApiKey::StreamsGroupTopologyDescriptionUpdate), (0, 0));
    assert_eq!(
        ApiKey::try_from(93).unwrap(),
        ApiKey::StreamsGroupTopologyDescriptionUpdate
    );
}

#[test]
fn a_heartbeat_with_a_topology_and_tasks_is_read_back_whole_at_both_versions() {
    let mut req = StreamsGroupHeartbeatRequest::default();
    req.group_id = StrBytes::from_static_str("app").into();
    req.member_id = StrBytes::from_static_str("m-1");
    req.rebalance_timeout_ms = 30_000;
    let mut topology = Topology::default();
    topology.epoch = 2;
    topology.subtopologies = vec![Default::default()];
    req.topology = Some(topology);
    let mut tasks = TaskIds::default();
    tasks.subtopology_id = StrBytes::from_static_str("0");
    tasks.partitions = vec![0, 2];
    req.active_tasks = Some(vec![tasks]);
    let mut endpoint = Endpoint::default();
    endpoint.host = StrBytes::from_static_str("localhost");
    endpoint.port = 65_535;
    req.user_endpoint = Some(endpoint);
    req.shutdown_application = true;
    for version in 0..=1 {
        let read: StreamsGroupHeartbeatRequest = back(bytes(&req, version), version);
        assert_eq!(read, req, "v{version}");
    }
}

/// Version 0 carries the recovery lag as an int32 (`AcceptableRecoveryLagLegacy`); version 1 has an int64 in its place
/// and `TopologyDescriptionRequired` (KIP-1331): four bytes and a byte more. The long lag is ignorable (left out at 0);
/// the legacy one is not, so it cannot be written at 1.
#[test]
fn the_heartbeat_answer_has_the_legacy_lag_at_0_and_the_long_lag_and_description_flag_at_1() {
    let mut resp = StreamsGroupHeartbeatResponse::default();
    resp.acceptable_recovery_lag = 10_000;
    resp.task_offset_interval_ms = 60_000;
    let at0: StreamsGroupHeartbeatResponse = back(bytes(&resp, 0), 0);
    assert_eq!(
        (
            at0.acceptable_recovery_lag_legacy,
            at0.acceptable_recovery_lag
        ),
        (0, -1)
    );
    let at1: StreamsGroupHeartbeatResponse = back(bytes(&resp, 1), 1);
    assert_eq!(at1.acceptable_recovery_lag, 10_000);
    assert_eq!(bytes(&resp, 0).len() + 4 + 1, bytes(&resp, 1).len());
    resp.acceptable_recovery_lag_legacy = 7;
    assert!(resp.encode(&mut BytesMut::new(), 1).is_err());
}

#[test]
fn describe_v1_asks_for_the_topology_description_and_is_told_its_status() {
    let mut req = StreamsGroupDescribeRequest::default();
    req.group_ids = vec![StrBytes::from_static_str("app").into()];
    req.include_topology_description = true;
    let read: StreamsGroupDescribeRequest = back(bytes(&req, 1), 1);
    assert!(read.include_topology_description);
    assert!(req.encode(&mut BytesMut::new(), 0).is_err());
    let resp = StreamsGroupDescribeResponse::default();
    for version in 0..=1 {
        let _: StreamsGroupDescribeResponse = back(bytes(&resp, version), version);
    }
}

#[test]
fn the_topology_description_update_answer_carries_an_error_and_its_words() {
    let mut resp = StreamsGroupTopologyDescriptionUpdateResponse::default();
    resp.error_code = 35;
    resp.error_message = Some(StrBytes::from_static_str("no plugin"));
    let read: StreamsGroupTopologyDescriptionUpdateResponse = back(bytes(&resp, 0), 0);
    assert_eq!(read, resp);
}
