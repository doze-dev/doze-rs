//! The topic APIs under ACLs on a real socket (slice 8d-1): a client that logged in as an ordinary user is refused on a
//! topic it has no ACL for and served on one it has, through the whole stack (decode, session, caller, broker, encode).
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

use client::{Client, metadata_for};
use kafka_auth::{Authenticator, Credentials, Mechanism};
use kafka_broker::{Broker, Config as BrokerConfig};
use kafka_protocol::protocol::StrBytes;
use kafka_server::{Config, Running, Server};
use kafka_storage::sim::SimFs;
use kafka_wire::batch::build::BatchBuilder;
use kafka_wire::messages::create_acls_request::AclCreation;
use kafka_wire::messages::create_topics_request::CreatableTopic;
use kafka_wire::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_wire::messages::list_offsets_request::{ListOffsetsPartition, ListOffsetsTopic};
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::{
    ApiKey, CreateAclsRequest, CreateAclsResponse, CreateTopicsRequest, CreateTopicsResponse,
    FetchRequest, FetchResponse, ListOffsetsRequest, ListOffsetsResponse, MetadataResponse,
    ProduceRequest, ProduceResponse, TopicName,
};

const READ: i8 = 3;
const WRITE: i8 = 4;
const CREATE: i8 = 5;
const DESCRIBE: i8 = 8;

async fn start() -> Running {
    let users = ["admin", "alice", "bob"].map(|u| (u.to_owned(), format!("{u}-pw")));
    let auth = Authenticator::new(&[Mechanism::Plain], Credentials::new(users).unwrap()).unwrap();
    let config = Config {
        listen: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        sasl: Some(Arc::new(auth)),
        ..Config::default()
    };
    let mut broker = BrokerConfig::new("/data".into());
    broker.acl_default_allow = false;
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

fn topic_acl(name: &str, principal: &str, operation: i8) -> AclCreation {
    let mut c = AclCreation::default();
    c.resource_type = 2;
    c.resource_name = text(name);
    c.resource_pattern_type = 3;
    c.principal = text(principal);
    c.host = text("*");
    c.operation = operation;
    c.permission_type = 3;
    c
}

async fn create_topic(c: &mut Client, name: &str) -> i16 {
    let mut t = CreatableTopic::default();
    t.name = TopicName(text(name));
    t.num_partitions = 1;
    t.replication_factor = 1;
    let mut req = CreateTopicsRequest::default();
    req.topics.push(t);
    let r: CreateTopicsResponse = c.call(ApiKey::CreateTopics, 7, &req).await.unwrap();
    r.topics[0].error_code
}

async fn produce(c: &mut Client, topic: &str) -> i16 {
    let mut partition = PartitionProduceData::default();
    partition.records = Some(BatchBuilder::new().records(3).build());
    let mut data = TopicProduceData::default();
    data.name = TopicName(text(topic));
    data.partition_data.push(partition);
    let mut req = ProduceRequest::default();
    req.acks = 1;
    req.timeout_ms = 5000;
    req.topic_data.push(data);
    let r: ProduceResponse = c.call(ApiKey::Produce, 9, &req).await.unwrap();
    r.responses[0].partition_responses[0].error_code
}

/// The error and the number of record bytes of a fetch of the topic's partition 0 from the start.
async fn fetch(c: &mut Client, topic: &str) -> (i16, usize) {
    let mut p = FetchPartition::default();
    p.partition_max_bytes = 1 << 20;
    let mut t = FetchTopic::default();
    t.topic = TopicName(text(topic));
    t.partitions.push(p);
    let mut req = FetchRequest::default();
    req.max_bytes = 1 << 20;
    req.topics.push(t);
    let r: FetchResponse = c.call(ApiKey::Fetch, 12, &req).await.unwrap();
    let d = &r.responses[0].partitions[0];
    (d.error_code, d.records.as_ref().map_or(0, |b| b.len()))
}

async fn end_offset(c: &mut Client, topic: &str) -> (i16, i64) {
    let mut p = ListOffsetsPartition::default();
    p.timestamp = -1;
    p.current_leader_epoch = -1;
    let mut t = ListOffsetsTopic::default();
    t.name = TopicName(text(topic));
    t.partitions.push(p);
    let mut req = ListOffsetsRequest::default();
    req.topics.push(t);
    let r: ListOffsetsResponse = c.call(ApiKey::ListOffsets, 7, &req).await.unwrap();
    let p = &r.topics[0].partitions[0];
    (p.error_code, p.offset)
}

#[tokio::test]
async fn a_user_is_refused_on_a_topic_it_has_no_acl_for_and_served_on_one_it_has() {
    let server = start().await;
    let mut admin = login(&server, "admin").await;
    for t in ["mine", "theirs"] {
        assert_eq!(create_topic(&mut admin, t).await, 0);
        assert_eq!(
            produce(&mut admin, t).await,
            0,
            "the super user writes anywhere"
        );
    }
    let mut grant = CreateAclsRequest::default();
    grant.creations = vec![
        topic_acl("mine", "User:alice", READ),
        topic_acl("mine", "User:alice", WRITE),
        topic_acl("theirs", "User:bob", READ),
        topic_acl("new", "User:alice", CREATE),
    ];
    let r: CreateAclsResponse = admin.call(ApiKey::CreateAcls, 3, &grant).await.unwrap();
    assert!(r.results.iter().all(|x| x.error_code == 0));

    let mut alice = login(&server, "alice").await;
    // Produce, Fetch and ListOffsets: served on "mine" (READ and WRITE imply DESCRIBE), refused on "theirs".
    assert_eq!(produce(&mut alice, "mine").await, 0);
    assert_eq!(produce(&mut alice, "theirs").await, 29);
    let (code, bytes) = fetch(&mut alice, "mine").await;
    assert_eq!(code, 0);
    assert!(bytes > 0, "the records written come back");
    assert_eq!(fetch(&mut alice, "theirs").await, (29, 0));
    assert_eq!(end_offset(&mut alice, "mine").await, (0, 6));
    assert_eq!(end_offset(&mut alice, "theirs").await.0, 29);
    // CreateTopics: alice may create "new" and no other.
    assert_eq!(create_topic(&mut alice, "other").await, 29);
    assert_eq!(create_topic(&mut alice, "new").await, 0);
    // Bob reads "theirs" and writes nothing.
    let mut bob = login(&server, "bob").await;
    assert_eq!(fetch(&mut bob, "theirs").await.0, 0);
    assert_eq!(produce(&mut bob, "theirs").await, 29);
    assert_eq!(fetch(&mut bob, "mine").await.0, 29);
    // The refusals changed nothing: "theirs" holds the super user's batch only.
    assert_eq!(end_offset(&mut admin, "theirs").await, (0, 3));
    // Metadata agrees with the data APIs about what bob can see.
    let r: MetadataResponse = bob
        .call(ApiKey::Metadata, 9, &metadata_for("mine"))
        .await
        .unwrap();
    assert_eq!(r.topics[0].error_code, 29);
    let _ = DESCRIBE;
    server.shutdown().await;
}
