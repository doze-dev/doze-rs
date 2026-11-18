//! Produce v13 and TxnOffsetCommit v6 name their topics by id (KIP-516), as Kafka 4.4.0 serves them
//! (`KafkaApis.handleProduceRequest`, `KafkaApis.handleTxnOffsetCommitRequest`; docs/port/behaviours/transactions.md
//! TX-26, docs/port/behaviours/security.md SEC-334 and SEC-335). The versions below are held alongside: by name, as before.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
#[path = "support/authz_topics.rs"]
mod topics;

use kafka_broker::Caller;
use kafka_wire::StrBytes;
use kafka_wire::Uuid;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::txn_offset_commit_request::{
    TxnOffsetCommitRequestPartition, TxnOffsetCommitRequestTopic,
};
use kafka_wire::messages::{
    GroupId, ProduceRequest, ProduceResponse, ProducerId, TransactionalId, TxnOffsetCommitRequest,
    TxnOffsetCommitResponse,
};
use support::acls::{GROUP, LITERAL, READ, TXN_ID, WRITE, admin, creation, user};
use support::{Rig, Txn, produce_request, records, rig, topic, wire_name};
use topics::{allow, grant, rig_with_topics};

const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const TOPIC_AUTHORIZATION_FAILED: i16 = 29;
const TRANSACTIONAL_ID_AUTHORIZATION_FAILED: i16 = 53;
const UNKNOWN_TOPIC_ID: i16 = 100;

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

/// A Produce of one batch to each `(id, name, partition)`: the id is what v13 says, the name what v12 and below say.
fn produce_of(targets: &[(Uuid, &str, i32)]) -> ProduceRequest {
    let mut req = produce_request(-1, "", &[]);
    req.topic_data.clear();
    for (id, name, partition) in targets {
        let mut one = produce_request(-1, name, &[(*partition, records(1))]);
        one.topic_data[0].topic_id = *id;
        req.topic_data.extend(one.topic_data);
    }
    req
}

async fn produce(rig: &Rig, who: &Caller, version: i16, req: ProduceRequest) -> ProduceResponse {
    let Response::Produce(r) = rig.ask_as(who, version, Request::Produce(req)).await else {
        panic!("not Produce")
    };
    r
}

/// `(topic id, partition, code)` of each partition answered, in order.
fn produced(r: &ProduceResponse) -> Vec<(Uuid, i32, i16)> {
    r.responses
        .iter()
        .flat_map(|t| {
            t.partition_responses
                .iter()
                .map(|p| (t.topic_id, p.index, p.error_code))
        })
        .collect()
}

#[tokio::test]
async fn produce_v13_writes_to_the_topic_its_id_stands_for_and_an_id_that_stands_for_none_is_unknown_topic_id() {
    let rig = rig();
    let made = rig.create(vec![topic("t", 2)], false).await;
    let id = made[0].topic_id;
    let nobody = Uuid::from_u128(0xfeed);
    // A known id is written, its partition out of range is UNKNOWN_TOPIC_OR_PARTITION, an id no topic has and the nil id
    // are UNKNOWN_TOPIC_ID on each of their partitions, whatever name the entry also carries; all in one request.
    let req = produce_of(&[
        (id, "", 1),
        (id, "", 2),
        (nobody, "t", 0),
        (nobody, "t", 1),
        (Uuid::nil(), "t", 0),
    ]);
    let r = produce(&rig, &admin(), 13, req).await;
    assert_eq!(
        produced(&r),
        [
            (id, 1, 0),
            (id, 2, UNKNOWN_TOPIC_OR_PARTITION),
            (nobody, 0, UNKNOWN_TOPIC_ID),
            (nobody, 1, UNKNOWN_TOPIC_ID),
            (Uuid::nil(), 0, UNKNOWN_TOPIC_ID)
        ],
        "the answer names each topic by the id it was asked by"
    );
    assert_eq!(r.responses[0].partition_responses[0].base_offset, 0);
    // The record went where the id said: read back by name.
    assert_eq!(rig.produce(-1, "t", 1, records(1)).await.base_offset, 1);
}

/// The boundary: at v12 the entry is named by name; an unknown name is UNKNOWN_TOPIC_OR_PARTITION, as before, and the
/// same entries asked at v13 are judged by their ids.
#[tokio::test]
async fn produce_v12_names_by_name_and_v13_by_id() {
    let rig = rig();
    let made = rig.create(vec![topic("t", 1)], false).await;
    let id = made[0].topic_id;
    let req = || produce_of(&[(Uuid::from_u128(7), "t", 0), (id, "gone", 0)]);
    let codes = |r: &ProduceResponse| produced(r).iter().map(|p| p.2).collect::<Vec<_>>();
    assert_eq!(
        codes(&produce(&rig, &admin(), 12, req()).await),
        [0, UNKNOWN_TOPIC_OR_PARTITION]
    );
    assert_eq!(
        codes(&produce(&rig, &admin(), 13, req()).await),
        [UNKNOWN_TOPIC_ID, 0]
    );
}

/// WRITE is asked on the name the id stands for; an id that stands for none is UNKNOWN_TOPIC_ID to everyone, before any
/// right is asked (KafkaApis: the unknown ids are answered before `filterByAuthorized`).
#[tokio::test]
async fn produce_v13_asks_write_on_the_name_the_id_stands_for_and_an_unknown_id_is_unknown_to_all() {
    let rig = rig_with_topics(&["a", "b"]).await;
    grant(&rig, vec![allow("a", "User:alice", WRITE)]).await;
    let (a, b) = (rig.id_of("a").await, rig.id_of("b").await);
    let nobody = Uuid::from_u128(0xdead);
    let req = || produce_of(&[(a, "", 0), (b, "", 0), (nobody, "a", 0)]);
    let codes = |r: &ProduceResponse| produced(r).iter().map(|p| p.2).collect::<Vec<_>>();
    assert_eq!(
        codes(&produce(&rig, &user("alice"), 13, req()).await),
        [0, TOPIC_AUTHORIZATION_FAILED, UNKNOWN_TOPIC_ID]
    );
    assert_eq!(
        codes(&produce(&rig, &user("bob"), 13, req()).await),
        [
            TOPIC_AUTHORIZATION_FAILED,
            TOPIC_AUTHORIZATION_FAILED,
            UNKNOWN_TOPIC_ID
        ]
    );
}

/// A transactional Produce of v13 adds its partitions to the transaction as one of v12 does (TX-18), by the name its id
/// stands for; a partition of an unknown id is added to nothing.
#[tokio::test]
async fn a_transactional_produce_of_v13_adds_the_partitions_its_ids_stand_for() {
    let rig = rig();
    let made = rig.create(vec![topic("t", 2)], false).await;
    let id = made[0].topic_id;
    let p = rig.init_txn("tx", 60_000).await.unwrap();
    let batch = || {
        kafka_wire::batch::build::BatchBuilder::new()
            .records(1)
            .attributes(0x10)
            .timestamps(1_000, 1_000)
            .producer(p.producer, p.epoch, 0)
            .build()
    };
    let mut req = produce_request(-1, "", &[(1, batch())]);
    req.topic_data[0].topic_id = id;
    let mut unknown = produce_request(-1, "t", &[(0, batch())]);
    unknown.topic_data[0].topic_id = Uuid::from_u128(5);
    req.topic_data.extend(unknown.topic_data);
    req.transactional_id = Some(TransactionalId(text("tx")));
    let r = produce(&rig, &admin(), 13, req).await;
    assert_eq!(
        produced(&r),
        [(id, 1, 0), (Uuid::from_u128(5), 0, UNKNOWN_TOPIC_ID)]
    );
    let (state, _, parts) = rig.describe_txn("tx").await;
    assert_eq!((state.as_str(), parts), ("Ongoing", vec![("t".into(), 1)]));
}

/// A TxnOffsetCommit of one offset (5) per `(id, name, partition)`, as a committer that names no member.
fn commit_of(t: Txn, group: &str, targets: &[(Uuid, &str, i32)]) -> TxnOffsetCommitRequest {
    let mut req = TxnOffsetCommitRequest::default();
    req.transactional_id = TransactionalId(text("tx"));
    req.group_id = GroupId(text(group));
    req.producer_id = ProducerId(t.producer);
    req.producer_epoch = t.epoch;
    req.generation_id_or_member_epoch = -1;
    for (id, name, partition) in targets {
        let mut p = TxnOffsetCommitRequestPartition::default();
        p.partition_index = *partition;
        p.committed_offset = 5;
        p.committed_leader_epoch = -1;
        let mut topic = TxnOffsetCommitRequestTopic::default();
        topic.name = wire_name(name);
        topic.topic_id = *id;
        topic.partitions.push(p);
        req.topics.push(topic);
    }
    req
}

async fn commit(rig: &Rig, who: &Caller, version: i16, req: TxnOffsetCommitRequest) -> TxnOffsetCommitResponse {
    let Response::TxnOffsetCommit(r) = rig.ask_as(who, version, Request::TxnOffsetCommit(req)).await else {
        panic!("not TxnOffsetCommit")
    };
    r
}

/// `(topic id, partition, code)` of each partition answered, in order.
fn committed(r: &TxnOffsetCommitResponse) -> Vec<(Uuid, i32, i16)> {
    r.topics
        .iter()
        .flat_map(|t| {
            t.partitions
                .iter()
                .map(|p| (t.topic_id, p.partition_index, p.error_code))
        })
        .collect()
}

/// v6 stages the offsets of the topic its id stands for, adds the group as v5 does (TX-20), and they are applied when the
/// transaction commits; an id that stands for no topic is UNKNOWN_TOPIC_ID, a partition the topic has not
/// UNKNOWN_TOPIC_OR_PARTITION, and the answer names each topic by the id it was asked by.
#[tokio::test]
async fn txn_offset_commit_v6_stages_the_offsets_of_the_topic_its_id_stands_for() {
    let rig = rig();
    let made = rig.create(vec![topic("t", 1)], false).await;
    let id = made[0].topic_id;
    let nobody = Uuid::from_u128(0xbee);
    let p = rig.init_txn("tx", 60_000).await.unwrap();
    let req = commit_of(p, "g", &[(id, "", 0), (id, "", 3), (nobody, "t", 0), (Uuid::nil(), "", 0)]);
    let r = commit(&rig, &admin(), 6, req).await;
    assert_eq!(
        committed(&r),
        [
            (id, 0, 0),
            (id, 3, UNKNOWN_TOPIC_OR_PARTITION),
            (nobody, 0, UNKNOWN_TOPIC_ID),
            (Uuid::nil(), 0, UNKNOWN_TOPIC_ID)
        ]
    );
    let (state, _, parts) = rig.describe_txn("tx").await;
    assert_eq!(state, "Ongoing", "the group was added");
    assert!(parts.iter().all(|(t, _)| t == "__consumer_offsets"), "{parts:?}");
    rig.end_v5("tx", p, true).await;
    let offsets = rig.fetch_offsets(false, "g", Some(&[("t", &[0])])).await;
    assert_eq!(offsets.first().map(|o| o.2), Some(5), "applied by the commit, to t by its id");
}

/// The boundary: v5 names the topic by name (an empty name is no topic), v6 by id (the name is not on its wire).
#[tokio::test]
async fn txn_offset_commit_v5_names_by_name_and_v6_by_id() {
    let rig = rig();
    let made = rig.create(vec![topic("t", 1)], false).await;
    let id = made[0].topic_id;
    let p = rig.init_txn("tx", 60_000).await.unwrap();
    let req = || commit_of(p, "g", &[(id, "", 0), (Uuid::from_u128(3), "t", 0)]);
    let codes = |r: &TxnOffsetCommitResponse| committed(r).iter().map(|p| p.2).collect::<Vec<_>>();
    assert_eq!(
        codes(&commit(&rig, &admin(), 5, req()).await),
        [UNKNOWN_TOPIC_OR_PARTITION, 0]
    );
    assert_eq!(
        codes(&commit(&rig, &admin(), 6, req()).await),
        [0, UNKNOWN_TOPIC_ID]
    );
}

/// The rights of v6, in Kafka's order: WRITE on the transactional id and READ on the group refuse the whole request
/// (an unknown id included); then an id that stands for no topic is UNKNOWN_TOPIC_ID before its READ is asked; then READ
/// on the name a known id stands for.
#[tokio::test]
async fn txn_offset_commit_v6_asks_its_rights_in_kafkas_order() {
    let rig = rig_with_topics(&["a", "b"]).await;
    let (a, b) = (rig.id_of("a").await, rig.id_of("b").await);
    let nobody = Uuid::from_u128(0xdead);
    grant(
        &rig,
        vec![
            creation((TXN_ID, "tx", LITERAL), "User:alice", WRITE, 3),
            creation((GROUP, "g", LITERAL), "User:alice", READ, 3),
            allow("a", "User:alice", READ),
            creation((TXN_ID, "tx", LITERAL), "User:carol", WRITE, 3),
        ],
    )
    .await;
    let p = rig.init_txn_as_admin("tx").await;
    let req = || commit_of(p, "g", &[(a, "", 0), (b, "", 0), (nobody, "", 0)]);
    let codes = |r: &TxnOffsetCommitResponse| committed(r).iter().map(|p| p.2).collect::<Vec<_>>();
    assert_eq!(
        codes(&commit(&rig, &user("alice"), 6, req()).await),
        [0, TOPIC_AUTHORIZATION_FAILED, UNKNOWN_TOPIC_ID]
    );
    assert_eq!(
        codes(&commit(&rig, &user("bob"), 6, req()).await),
        [TRANSACTIONAL_ID_AUTHORIZATION_FAILED; 3]
    );
    assert_eq!(
        codes(&commit(&rig, &user("carol"), 6, req()).await),
        [30; 3],
        "GROUP_AUTHORIZATION_FAILED"
    );
}

impl support::Rig {
    /// InitProducerId for `id` as the super user, with a minute's timeout.
    async fn init_txn_as_admin(&self, id: &str) -> Txn {
        let mut req = kafka_wire::messages::InitProducerIdRequest::default();
        req.transactional_id = Some(TransactionalId(text(id)));
        req.transaction_timeout_ms = 60_000;
        let Response::InitProducerId(r) = self.ask_as(&admin(), 4, Request::InitProducerId(req)).await else {
            panic!("not InitProducerId")
        };
        assert_eq!(r.error_code, 0);
        Txn {
            producer: r.producer_id.0,
            epoch: r.producer_epoch,
        }
    }
}
