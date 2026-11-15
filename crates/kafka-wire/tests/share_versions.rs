//! The share-group messages at the versions Apache Kafka 4.4.0 has (vendor/kafka-protocol/PATCHES.md, patch 3): each
//! field the patch adds is written and read back at its versions, and is not on the wire below them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use bytes::{Bytes, BytesMut};
use kafka_protocol::messages::describe_share_group_offsets_response::{
    DescribeShareGroupOffsetsResponsePartition, DescribeShareGroupOffsetsResponseTopic,
};
use kafka_protocol::messages::{
    ApiKey, DescribeShareGroupOffsetsResponse, ShareAcknowledgeRequest, ShareAcknowledgeResponse,
    ShareFetchRequest,
};
use kafka_protocol::protocol::{Decodable, Encodable};

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
    assert_eq!(range(ApiKey::ShareFetch), (1, 2));
    assert_eq!(range(ApiKey::ShareAcknowledge), (1, 2));
    assert_eq!(range(ApiKey::DescribeShareGroupOffsets), (0, 1));
    assert_eq!(range(ApiKey::ShareGroupHeartbeat), (1, 1));
}

#[test]
fn share_fetch_v2_carries_the_acquire_mode_and_the_renew_flag() {
    let mut req = ShareFetchRequest::default();
    req.max_records = 500;
    req.batch_size = 100;
    req.share_acquire_mode = 1;
    req.is_renew_ack = true;
    let read: ShareFetchRequest = back(bytes(&req, 2), 2);
    assert_eq!((read.share_acquire_mode, read.is_renew_ack), (1, true));
    assert_eq!((read.max_records, read.batch_size), (500, 100));
    // Version 1 has neither: two bytes fewer, read back as the defaults; a renew flag cannot be written at 1.
    req.is_renew_ack = false;
    assert_eq!(bytes(&req, 1).len() + 2, bytes(&req, 2).len());
    let read: ShareFetchRequest = back(bytes(&req, 1), 1);
    assert_eq!((read.share_acquire_mode, read.is_renew_ack), (0, false));
    req.is_renew_ack = true;
    assert!(req.encode(&mut BytesMut::new(), 1).is_err());
}

#[test]
fn share_acknowledge_v2_carries_the_renew_flag_and_answers_the_lock_time() {
    let mut req = ShareAcknowledgeRequest::default();
    req.share_session_epoch = 3;
    req.is_renew_ack = true;
    let read: ShareAcknowledgeRequest = back(bytes(&req, 2), 2);
    assert!(read.is_renew_ack);
    assert_eq!(read.share_session_epoch, 3);
    assert!(req.encode(&mut BytesMut::new(), 1).is_err());

    let mut resp = ShareAcknowledgeResponse::default();
    resp.acquisition_lock_timeout_ms = 30_000;
    let read: ShareAcknowledgeResponse = back(bytes(&resp, 2), 2);
    assert_eq!(read.acquisition_lock_timeout_ms, 30_000);
    let read: ShareAcknowledgeResponse = back(bytes(&resp, 1), 1);
    assert_eq!(read.acquisition_lock_timeout_ms, 0, "not on the wire at 1");
}

#[test]
fn describe_share_group_offsets_v1_carries_the_lag() {
    let mut p = DescribeShareGroupOffsetsResponsePartition::default();
    assert_eq!(p.lag, -1, "Kafka's default");
    p.start_offset = 4;
    p.lag = 7;
    let mut t = DescribeShareGroupOffsetsResponseTopic::default();
    t.partitions.push(p);
    let mut resp = DescribeShareGroupOffsetsResponse::default();
    let mut g = kafka_protocol::messages::describe_share_group_offsets_response::DescribeShareGroupOffsetsResponseGroup::default();
    g.topics.push(t);
    resp.groups.push(g);
    let read: DescribeShareGroupOffsetsResponse = back(bytes(&resp, 1), 1);
    assert_eq!(read.groups[0].topics[0].partitions[0].lag, 7);
    let read: DescribeShareGroupOffsetsResponse = back(bytes(&resp, 0), 0);
    assert_eq!(
        read.groups[0].topics[0].partitions[0].lag, -1,
        "not on the wire at 0"
    );
}
