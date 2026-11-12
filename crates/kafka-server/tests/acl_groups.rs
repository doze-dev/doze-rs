//! The group and transaction ACLs on a real socket: two clients logged in by SASL as users with different ACLs, one
//! allowed to join a group and read a topic and one refused, and a transactional producer with and without WRITE on its
//! transactional id.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/sasl_client.rs"]
mod client;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use client::{Client, metadata_for};
use kafka_auth::{Authenticator, Credentials, Mechanism};
use kafka_broker::{Broker, Config as BrokerConfig};
use kafka_protocol::protocol::StrBytes;
use kafka_server::{Config, Running, Server};
use kafka_storage::sim::SimFs;
use kafka_wire::messages::add_partitions_to_txn_request::AddPartitionsToTxnTopic;
use kafka_wire::messages::create_acls_request::AclCreation;
use kafka_wire::messages::join_group_request::JoinGroupRequestProtocol;
use kafka_wire::messages::offset_commit_request::{
    OffsetCommitRequestPartition, OffsetCommitRequestTopic,
};
use kafka_wire::messages::{
    AddPartitionsToTxnRequest, AddPartitionsToTxnResponse, ApiKey, CreateAclsRequest,
    CreateAclsResponse, EndTxnRequest, EndTxnResponse, FindCoordinatorRequest,
    FindCoordinatorResponse, GroupId, InitProducerIdRequest, InitProducerIdResponse,
    JoinGroupRequest, JoinGroupResponse, ListGroupsRequest, ListGroupsResponse, MetadataResponse,
    OffsetCommitRequest, OffsetCommitResponse, ProducerId, TopicName, TransactionalId,
};

const TOPIC: i8 = 2;
const GROUP: i8 = 3;
const TXN_ID: i8 = 5;
const READ: i8 = 3;
const WRITE: i8 = 4;
const DESCRIBE: i8 = 8;

async fn start() -> Running {
    let users = [
        ("admin", "admin-pw"),
        ("alice", "alice-pw"),
        ("bob", "bob-pw"),
    ]
    .map(|(u, p)| (u.to_owned(), p.to_owned()));
    let auth = Authenticator::new(&[Mechanism::Plain], Credentials::new(users).unwrap()).unwrap();
    let config = Config {
        listen: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        sasl: Some(Arc::new(auth)),
        ..Config::default()
    };
    let mut broker = BrokerConfig::new("/data".into());
    broker.acl_default_allow = false;
    broker.group_initial_rebalance_delay = Duration::ZERO;
    broker.super_users.insert("User:admin".to_owned());
    let broker = Broker::open(SimFs::new(), broker).unwrap();
    Server::bind(config, Arc::new(broker)).await.unwrap()
}

async fn login(server: &Running, user: &str) -> Client {
    let mut c = Client::connect(server).await;
    c.log_in(format!("\0{user}\0{user}-pw").as_bytes()).await;
    c
}

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

fn allow(resource: (i8, &str), principal: &str, operation: i8) -> AclCreation {
    let mut c = AclCreation::default();
    c.resource_type = resource.0;
    c.resource_name = text(resource.1);
    c.resource_pattern_type = 3;
    c.principal = text(principal);
    c.host = text("*");
    c.operation = operation;
    c.permission_type = 3;
    c
}

async fn grant(c: &mut Client, acls: Vec<AclCreation>) {
    let mut req = CreateAclsRequest::default();
    req.creations = acls;
    let r: CreateAclsResponse = c.call(ApiKey::CreateAcls, 3, &req).await.unwrap();
    assert!(r.results.iter().all(|x| x.error_code == 0), "{r:?}");
}

/// The topic `t` made by the super user (the first ask is answered "unknown" and creates).
async fn make_topic(admin: &mut Client, t: &str) {
    for expect in [3, 0] {
        let r: MetadataResponse = admin
            .call(ApiKey::Metadata, 9, &metadata_for(t))
            .await
            .unwrap();
        assert_eq!(r.topics[0].error_code, expect);
    }
}

async fn find_coordinator(c: &mut Client, key: &str, key_type: i8) -> i16 {
    let mut req = FindCoordinatorRequest::default();
    req.key_type = key_type;
    req.coordinator_keys = vec![text(key)];
    let r: FindCoordinatorResponse = c.call(ApiKey::FindCoordinator, 4, &req).await.unwrap();
    r.coordinators[0].error_code
}

async fn join(c: &mut Client, group: &str) -> JoinGroupResponse {
    let mut req = JoinGroupRequest::default();
    req.group_id = GroupId(text(group));
    req.session_timeout_ms = 10_000;
    req.rebalance_timeout_ms = 30_000;
    req.protocol_type = text("consumer");
    let mut p = JoinGroupRequestProtocol::default();
    p.name = text("range");
    req.protocols = vec![p];
    c.call(ApiKey::JoinGroup, 3, &req).await.unwrap()
}

async fn commit(c: &mut Client, group: &str, topic: &str) -> i16 {
    let mut req = OffsetCommitRequest::default();
    req.group_id = GroupId(text(group));
    req.generation_id_or_member_epoch = -1;
    let mut p = OffsetCommitRequestPartition::default();
    p.partition_index = 0;
    p.committed_offset = 5;
    let mut t = OffsetCommitRequestTopic::default();
    t.name = TopicName(text(topic));
    t.partitions = vec![p];
    req.topics = vec![t];
    let r: OffsetCommitResponse = c.call(ApiKey::OffsetCommit, 8, &req).await.unwrap();
    r.topics[0].partitions[0].error_code
}

#[tokio::test]
async fn a_user_allowed_to_join_and_read_is_served_and_one_with_nothing_is_refused() {
    let server = start().await;
    let mut admin = login(&server, "admin").await;
    make_topic(&mut admin, "t").await;
    grant(
        &mut admin,
        vec![
            allow((GROUP, "g"), "User:alice", READ),
            allow((GROUP, "g"), "User:alice", DESCRIBE),
            allow((TOPIC, "t"), "User:alice", READ),
        ],
    )
    .await;
    let mut alice = login(&server, "alice").await;
    let mut bob = login(&server, "bob").await;

    assert_eq!(find_coordinator(&mut alice, "g", 0).await, 0);
    assert_eq!(find_coordinator(&mut bob, "g", 0).await, 30);
    // A commit by no member (before the group has any); alice may read t but not u.
    assert_eq!(commit(&mut alice, "g", "t").await, 0);
    assert_eq!(commit(&mut bob, "g", "t").await, 30);
    assert_eq!(commit(&mut alice, "g", "u").await, 29);
    let joined = join(&mut alice, "g").await;
    assert_eq!((joined.error_code, joined.generation_id), (0, 1));
    let refused = join(&mut bob, "g").await;
    assert_eq!((refused.error_code, refused.generation_id), (30, -1));

    // ListGroups shows each only what it may describe: alice the group her commit made, bob nothing.
    let listed = |r: ListGroupsResponse| {
        r.groups
            .iter()
            .map(|g| g.group_id.to_string())
            .collect::<Vec<_>>()
    };
    let r: ListGroupsResponse = alice
        .call(ApiKey::ListGroups, 5, &ListGroupsRequest::default())
        .await
        .unwrap();
    assert_eq!(listed(r), ["g"]);
    let r: ListGroupsResponse = bob
        .call(ApiKey::ListGroups, 5, &ListGroupsRequest::default())
        .await
        .unwrap();
    assert!(listed(r).is_empty());
    server.shutdown().await;
}

async fn init(c: &mut Client, id: &str) -> InitProducerIdResponse {
    let mut req = InitProducerIdRequest::default();
    req.transactional_id = Some(TransactionalId(text(id)));
    req.transaction_timeout_ms = 60_000;
    c.call(ApiKey::InitProducerId, 4, &req).await.unwrap()
}

#[tokio::test]
async fn a_transactional_producer_needs_write_on_its_transactional_id() {
    let server = start().await;
    let mut admin = login(&server, "admin").await;
    make_topic(&mut admin, "t").await;
    grant(
        &mut admin,
        vec![
            allow((TXN_ID, "tid"), "User:alice", WRITE),
            allow((TOPIC, "t"), "User:alice", WRITE),
        ],
    )
    .await;
    let mut alice = login(&server, "alice").await;
    let mut bob = login(&server, "bob").await;

    let refused = init(&mut bob, "tid").await;
    assert_eq!((refused.error_code, refused.producer_id.0), (53, -1));
    let producer = init(&mut alice, "tid").await;
    assert_eq!(producer.error_code, 0);

    let mut add = AddPartitionsToTxnRequest::default();
    add.v3_and_below_transactional_id = TransactionalId(text("tid"));
    add.v3_and_below_producer_id = ProducerId(producer.producer_id.0);
    add.v3_and_below_producer_epoch = producer.producer_epoch;
    let mut topic = AddPartitionsToTxnTopic::default();
    topic.name = TopicName(text("t"));
    topic.partitions = vec![0];
    add.v3_and_below_topics = vec![topic];
    let code = |r: AddPartitionsToTxnResponse| {
        r.results_by_topic_v3_and_below[0].results_by_partition[0].partition_error_code
    };
    let r: AddPartitionsToTxnResponse =
        bob.call(ApiKey::AddPartitionsToTxn, 3, &add).await.unwrap();
    assert_eq!(code(r), 53);
    let r: AddPartitionsToTxnResponse = alice
        .call(ApiKey::AddPartitionsToTxn, 3, &add)
        .await
        .unwrap();
    assert_eq!(code(r), 0);

    let mut end = EndTxnRequest::default();
    end.transactional_id = TransactionalId(text("tid"));
    end.producer_id = ProducerId(producer.producer_id.0);
    end.producer_epoch = producer.producer_epoch;
    end.committed = true;
    let r: EndTxnResponse = bob.call(ApiKey::EndTxn, 3, &end).await.unwrap();
    assert_eq!(r.error_code, 53);
    let r: EndTxnResponse = alice.call(ApiKey::EndTxn, 3, &end).await.unwrap();
    assert_eq!(r.error_code, 0);
    server.shutdown().await;
}
