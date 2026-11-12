//! ACLs on a real socket: the principal a login proves is the one the broker authorizes, and a connection that
//! never logged in is `User:ANONYMOUS`.
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
use kafka_wire::messages::create_acls_request::AclCreation;
use kafka_wire::messages::{
    ApiKey, CreateAclsRequest, CreateAclsResponse, DescribeAclsRequest, DescribeAclsResponse,
    MetadataResponse,
};

async fn start(sasl: bool, default_allow: bool) -> Running {
    let users = [
        ("admin", "admin-pw"),
        ("alice", "alice-pw"),
        ("bob", "bob-pw"),
    ]
    .map(|(u, p)| (u.to_owned(), p.to_owned()));
    let auth = Authenticator::new(&[Mechanism::Plain], Credentials::new(users).unwrap()).unwrap();
    let config = Config {
        listen: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        sasl: sasl.then(|| Arc::new(auth)),
        ..Config::default()
    };
    let mut broker = BrokerConfig::new("/data".into());
    broker.acl_default_allow = default_allow;
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

async fn topic_error(c: &mut Client, topic: &str) -> i16 {
    let r: MetadataResponse = c
        .call(ApiKey::Metadata, 9, &metadata_for(topic))
        .await
        .unwrap();
    r.topics[0].error_code
}

#[tokio::test]
async fn what_a_login_proves_is_what_the_acls_are_checked_against() {
    let server = start(true, false).await;
    let mut admin = login(&server, "admin").await;
    // The super user makes the topics (the first ask is answered "unknown" and creates), and grants a right each.
    for t in ["for-alice", "for-bob"] {
        assert_eq!(topic_error(&mut admin, t).await, 3);
        assert_eq!(topic_error(&mut admin, t).await, 0);
    }
    grant(
        &mut admin,
        vec![
            allow((2, "for-alice"), "User:alice", 8),
            allow((2, "for-bob"), "User:bob", 8),
        ],
    )
    .await;
    let mut alice = login(&server, "alice").await;
    let mut bob = login(&server, "bob").await;
    assert_eq!(topic_error(&mut alice, "for-alice").await, 0);
    assert_eq!(topic_error(&mut alice, "for-bob").await, 29);
    assert_eq!(topic_error(&mut bob, "for-alice").await, 29);
    assert_eq!(topic_error(&mut bob, "for-bob").await, 0);
    // Alice may not use the ACL APIs: 31.
    let mut req = CreateAclsRequest::default();
    req.creations = vec![allow((2, "for-bob"), "User:alice", 3)];
    let r: CreateAclsResponse = alice.call(ApiKey::CreateAcls, 3, &req).await.unwrap();
    assert_eq!(r.results[0].error_code, 31);
    // A raw code of 0 closes the connection.
    let mut zero = DescribeAclsRequest::default();
    (
        zero.resource_name_filter,
        zero.principal_filter,
        zero.host_filter,
    ) = (None, None, None);
    (
        zero.resource_type_filter,
        zero.pattern_type_filter,
        zero.operation,
        zero.permission_type,
    ) = (0, 3, 1, 1);
    alice.send(ApiKey::DescribeAcls, 3, &zero).await;
    assert!(alice.is_closed().await);
    server.shutdown().await;
}

#[tokio::test]
async fn without_sasl_the_connection_is_the_anonymous_user_and_the_acls_bind_it() {
    let server = start(false, true).await;
    let mut c = Client::connect(&server).await;
    // Nothing stored and the default allows: the ACL APIs work for anyone.
    let none = {
        let mut req = DescribeAclsRequest::default();
        (
            req.resource_name_filter,
            req.principal_filter,
            req.host_filter,
        ) = (None, None, None);
        (
            req.resource_type_filter,
            req.pattern_type_filter,
            req.operation,
            req.permission_type,
        ) = (1, 1, 1, 1);
        req
    };
    let r: DescribeAclsResponse = c.call(ApiKey::DescribeAcls, 3, &none).await.unwrap();
    assert_eq!((r.error_code, r.resources.len()), (0, 0));
    // An ACL on a topic for ANONYMOUS, and one on the topic for somebody else.
    grant(
        &mut c,
        vec![
            allow((2, "mine"), "User:ANONYMOUS", 8),
            allow((2, "theirs"), "User:alice", 8),
        ],
    )
    .await;
    assert_eq!(
        topic_error(&mut c, "mine").await,
        3,
        "described, and not there yet"
    );
    assert_eq!(topic_error(&mut c, "theirs").await, 29);
    // A cluster ACL for somebody else turns the default off for the cluster: the anonymous user is locked out
    // of the ACL APIs, which is what a deny default would have meant without SASL.
    grant(&mut c, vec![allow((4, "kafka-cluster"), "User:alice", 7)]).await;
    let r: DescribeAclsResponse = c.call(ApiKey::DescribeAcls, 3, &none).await.unwrap();
    assert_eq!(r.error_code, 31);
    server.shutdown().await;
}
