//! ACL host matching over a real socket, for an IPv4 and an IPv6 client (SEC-17, SEC-222): an ACL's host is compared as text
//! with the peer's address as Java's `InetAddress.getHostAddress` writes it, so `127.0.0.1` matches an IPv4 peer and
//! `0:0:0:0:0:0:0:1` an IPv6 one, and `::1` matches neither. The IPv6 test is skipped (and says so) on a machine with no
//! IPv6 loopback.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/sasl_client.rs"]
mod client;

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use client::{Client, metadata_for};
use kafka_broker::{Broker, Config as BrokerConfig};
use kafka_protocol::protocol::StrBytes;
use kafka_server::{Config, Running, Server};
use kafka_storage::sim::SimFs;
use kafka_wire::messages::create_acls_request::AclCreation;
use kafka_wire::messages::{ApiKey, CreateAclsRequest, CreateAclsResponse, MetadataResponse};

async fn start(listen: SocketAddr) -> Option<Running> {
    let config = Config {
        listen,
        sasl: None,
        ..Config::default()
    };
    let mut broker = BrokerConfig::new("/data".into());
    broker.acl_default_allow = true;
    let broker = Broker::open(SimFs::new(), broker).unwrap();
    Server::bind(config, Arc::new(broker)).await.ok()
}

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

fn deny_describe(topic: &str, host: &str) -> AclCreation {
    let mut c = AclCreation::default();
    c.resource_type = 2;
    c.resource_name = text(topic);
    c.resource_pattern_type = 3;
    c.principal = text("User:ANONYMOUS");
    c.host = text(host);
    c.operation = 8;
    c.permission_type = 2;
    c
}

fn allow_describe(topic: &str) -> AclCreation {
    let mut c = deny_describe(topic, "*");
    c.permission_type = 3;
    c
}

async fn code(c: &mut Client, topic: &str) -> i16 {
    let r: MetadataResponse = c
        .call(ApiKey::Metadata, 9, &metadata_for(topic))
        .await
        .unwrap();
    r.topics[0].error_code
}

async fn deny(c: &mut Client, acls: Vec<AclCreation>) {
    let mut req = CreateAclsRequest::default();
    req.creations = acls
        .iter()
        .map(|a| allow_describe(&a.resource_name))
        .chain(acls.iter().cloned())
        .collect();
    let r: CreateAclsResponse = c.call(ApiKey::CreateAcls, 3, &req).await.unwrap();
    assert!(r.results.iter().all(|x| x.error_code == 0), "{r:?}");
}

#[tokio::test]
async fn an_ipv4_peer_is_matched_as_dotted_decimal() {
    let server = start(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .unwrap();
    let mut c = Client::connect(&server).await;
    deny(
        &mut c,
        vec![
            deny_describe("a", "127.0.0.1"),
            deny_describe("b", "0:0:0:0:0:0:0:1"),
            deny_describe("c", "::1"),
            deny_describe("d", "10.9.9.9"),
            deny_describe("e", "*"),
        ],
    )
    .await;
    assert_eq!(
        code(&mut c, "a").await,
        29,
        "127.0.0.1 matches the IPv4 peer"
    );
    assert_eq!(
        code(&mut c, "b").await,
        3,
        "an IPv6 host does not match an IPv4 peer"
    );
    assert_eq!(code(&mut c, "c").await, 3);
    assert_eq!(code(&mut c, "d").await, 3, "another host does not match");
    assert_eq!(code(&mut c, "e").await, 29, "* matches");
    server.shutdown().await;
}

#[tokio::test]
async fn an_ipv6_peer_is_matched_as_eight_groups_without_shortening() {
    let Some(server) = start(SocketAddr::from((Ipv6Addr::LOCALHOST, 0))).await else {
        eprintln!("skipped: this machine has no IPv6 loopback to listen on");
        return;
    };
    let mut c = Client::connect(&server).await;
    deny(
        &mut c,
        vec![
            deny_describe("a", "127.0.0.1"),
            deny_describe("b", "0:0:0:0:0:0:0:1"),
            deny_describe("c", "::1"),
        ],
    )
    .await;
    assert_eq!(code(&mut c, "a").await, 3);
    assert_eq!(
        code(&mut c, "b").await,
        29,
        "Java writes ::1 as 0:0:0:0:0:0:0:1"
    );
    assert_eq!(
        code(&mut c, "c").await,
        3,
        "Kafka compares text: ::1 never matches"
    );
    server.shutdown().await;
}
