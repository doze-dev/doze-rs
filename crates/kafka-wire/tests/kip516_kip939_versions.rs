//! TxnOffsetCommit v6 (topic ids, KIP-516) and InitProducerId v6 (two-phase commit, KIP-939) at the versions Apache Kafka
//! 4.4.0 has (vendor/kafka-protocol/PATCHES.md, patch 6): each field that differs between versions is written and read back
//! where it is, and is not on the wire where it is not. Produce v13 was in 0.18.0 already; its id is held here too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use bytes::{Bytes, BytesMut};
use kafka_protocol::messages::produce_request::TopicProduceData;
use kafka_protocol::messages::txn_offset_commit_request::{
    TxnOffsetCommitRequestPartition, TxnOffsetCommitRequestTopic,
};
use kafka_protocol::messages::txn_offset_commit_response::{
    TxnOffsetCommitResponsePartition, TxnOffsetCommitResponseTopic,
};
use kafka_protocol::messages::{
    ApiKey, InitProducerIdRequest, InitProducerIdResponse, ProduceRequest, ProducerId, TopicName,
    TransactionalId, TxnOffsetCommitRequest, TxnOffsetCommitResponse,
};
use kafka_protocol::protocol::{Decodable, Encodable, StrBytes};
use uuid::Uuid;

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

fn name(s: &'static str) -> TopicName {
    TopicName(StrBytes::from_static_str(s))
}

const ID: Uuid = Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);

#[test]
fn the_keys_are_at_kafka_4_4_versions() {
    let range = |k: ApiKey| (k.valid_versions().min, k.valid_versions().max);
    assert_eq!(
        range(ApiKey::Produce),
        (0, 13),
        "v0 to v2 since patch 8, for the older profiles"
    );
    assert_eq!(range(ApiKey::TxnOffsetCommit), (0, 6));
    assert_eq!(range(ApiKey::InitProducerId), (0, 6));
}

fn commit_request() -> TxnOffsetCommitRequest {
    let mut p = TxnOffsetCommitRequestPartition::default();
    p.partition_index = 2;
    p.committed_offset = 41;
    let mut t = TxnOffsetCommitRequestTopic::default();
    t.name = name("orders");
    t.topic_id = ID;
    t.partitions = vec![p];
    let mut req = TxnOffsetCommitRequest::default();
    req.transactional_id = TransactionalId(StrBytes::from_static_str("tx"));
    req.group_id = kafka_protocol::messages::GroupId(StrBytes::from_static_str("g"));
    req.producer_id = ProducerId(7);
    req.generation_id_or_member_epoch = 3;
    req.topics = vec![t];
    req
}

/// v6 carries the topic's id and not its name; v5 and below the name and not the id.
#[test]
fn a_txn_offset_commit_names_its_topic_by_id_from_v6_and_by_name_before() {
    let req = commit_request();
    let at6: TxnOffsetCommitRequest = back(bytes(&req, 6), 6);
    assert_eq!(at6.topics[0].topic_id, ID);
    assert_eq!(at6.topics[0].name, name(""), "no name on the wire at v6");
    assert_eq!(at6.topics[0].partitions[0].committed_offset, 41);
    assert_eq!(at6.generation_id_or_member_epoch, 3);
    let at5: TxnOffsetCommitRequest = back(bytes(&req, 5), 5);
    assert_eq!(at5.topics[0].name, name("orders"));
    assert_eq!(
        at5.topics[0].topic_id,
        Uuid::nil(),
        "no id on the wire before v6"
    );
    // The name is ignorable at v6 and the id at v5: a request that sets both is written at either, less one.
    assert!(bytes(&req, 6).len() > bytes(&req, 5).len());
}

#[test]
fn a_txn_offset_commit_answer_carries_the_id_from_v6_and_the_name_before() {
    let mut p = TxnOffsetCommitResponsePartition::default();
    p.partition_index = 2;
    p.error_code = 100;
    let mut t = TxnOffsetCommitResponseTopic::default();
    t.name = name("orders");
    t.topic_id = ID;
    t.partitions = vec![p];
    let mut resp = TxnOffsetCommitResponse::default();
    resp.topics = vec![t];
    let at6: TxnOffsetCommitResponse = back(bytes(&resp, 6), 6);
    assert_eq!(
        (at6.topics[0].topic_id, &at6.topics[0].name),
        (ID, &name(""))
    );
    assert_eq!(at6.topics[0].partitions[0].error_code, 100);
    let at5: TxnOffsetCommitResponse = back(bytes(&resp, 5), 5);
    assert_eq!(
        (at5.topics[0].topic_id, &at5.topics[0].name),
        (Uuid::nil(), &name("orders"))
    );
}

/// v6's two flags are read and written there, and a request that sets either cannot be written below it (the fields are
/// not ignorable: a client must not send a two-phase commit to a broker that does not know one).
#[test]
fn init_producer_id_v6_carries_the_two_phase_commit_flags_and_older_versions_cannot() {
    let mut req = InitProducerIdRequest::default();
    req.transactional_id = Some(TransactionalId(StrBytes::from_static_str("tx")));
    req.transaction_timeout_ms = 60_000;
    req.enable_2_pc = true;
    req.keep_prepared_txn = true;
    let at6: InitProducerIdRequest = back(bytes(&req, 6), 6);
    assert!(at6.enable_2_pc && at6.keep_prepared_txn);
    let mut buf = BytesMut::new();
    assert!(req.encode(&mut buf, 5).is_err(), "v5 has no 2PC flags");
    req.enable_2_pc = false;
    assert!(
        req.encode(&mut BytesMut::new(), 5).is_err(),
        "nor KeepPreparedTxn"
    );
    req.keep_prepared_txn = false;
    let at5: InitProducerIdRequest = back(bytes(&req, 5), 5);
    assert!(!at5.enable_2_pc && !at5.keep_prepared_txn);
    // The two flags are one byte each, after the producer id and epoch.
    assert_eq!(bytes(&req, 6).len(), bytes(&req, 5).len() + 2);
}

#[test]
fn init_producer_id_v6_answers_with_the_ongoing_transaction_fields() {
    let resp = InitProducerIdResponse::default();
    assert_eq!(
        (
            resp.ongoing_txn_producer_id,
            resp.ongoing_txn_producer_epoch
        ),
        (ProducerId(-1), -1),
        "-1 when no transaction is kept"
    );
    let mut kept = resp.clone();
    kept.ongoing_txn_producer_id = ProducerId(9);
    kept.ongoing_txn_producer_epoch = 4;
    let at6: InitProducerIdResponse = back(bytes(&kept, 6), 6);
    assert_eq!(
        (at6.ongoing_txn_producer_id, at6.ongoing_txn_producer_epoch),
        (ProducerId(9), 4)
    );
    assert_eq!(bytes(&resp, 6).len(), bytes(&resp, 5).len() + 10);
}

#[test]
fn produce_names_its_topic_by_id_from_v13() {
    let mut t = TopicProduceData::default();
    t.name = name("orders");
    t.topic_id = ID;
    let mut req = ProduceRequest::default();
    req.topic_data = vec![t];
    let at13: ProduceRequest = back(bytes(&req, 13), 13);
    assert_eq!(
        (at13.topic_data[0].topic_id, &at13.topic_data[0].name),
        (ID, &name(""))
    );
    let at12: ProduceRequest = back(bytes(&req, 12), 12);
    assert_eq!(
        (at12.topic_data[0].topic_id, &at12.topic_data[0].name),
        (Uuid::nil(), &name("orders"))
    );
}
