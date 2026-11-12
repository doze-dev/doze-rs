//! TLS and time (feature `tls`): the handshake and the pre-auth budget, an ACL by host over TLS, and a stop with TLS connections open.
#![cfg(feature = "tls")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/sasl_client.rs"]
mod client;
#[path = "support/tls_support.rs"]
mod support;

use std::time::{Duration, Instant};

use client::{ALICE, Client, metadata_for};
use kafka_protocol::protocol::StrBytes;
use kafka_wire::messages::create_acls_request::AclCreation;
use kafka_wire::messages::{ApiKey, CreateAclsRequest, CreateAclsResponse, MetadataResponse};
use rustls::pki_types::ServerName;
use support::{Opts, connector, dial, start};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};

/// Everything the server sends to `socket` until it closes it (or `wait` runs out).
async fn drain(socket: &mut TcpStream, wait: Duration) -> Vec<u8> {
    let mut all = Vec::new();
    let _ = timeout(wait, socket.read_to_end(&mut all))
        .await
        .expect("the server closed the connection");
    all
}

#[tokio::test]
async fn a_connection_that_never_handshakes_is_cut_by_the_pre_auth_budget_and_does_not_block_others()
 {
    let server = start(Opts {
        pre_auth: Some(Duration::from_millis(400)),
        ..Opts::default()
    })
    .await;
    let began = Instant::now();
    let mut silent = TcpStream::connect(server.local_addr()).await.unwrap();
    // While it sits there the accept loop and the other connections are free.
    let mut c = dial(&server).await;
    assert!(c.versions().await.is_some());
    let said = drain(&mut silent, Duration::from_secs(5)).await;
    assert!(
        said.is_empty(),
        "nothing is written to a connection that never began"
    );
    let took = began.elapsed();
    assert!(
        took >= Duration::from_millis(350) && took < Duration::from_secs(3),
        "{took:?}"
    );
    server.shutdown().await;
}

#[tokio::test]
async fn the_handshake_counts_against_the_login_budget_it_does_not_extend_it() {
    let pre_auth = Duration::from_millis(900);
    let server = start(Opts {
        sasl: true,
        pre_auth: Some(pre_auth),
        ..Opts::default()
    })
    .await;
    // 500 ms before the handshake and a prompt login: inside the budget.
    let socket = TcpStream::connect(server.local_addr()).await.unwrap();
    sleep(Duration::from_millis(500)).await;
    let name = ServerName::try_from("localhost").unwrap();
    let stream = connector("root.pem")
        .connect(name.clone(), socket)
        .await
        .unwrap();
    let mut prompt = Client::over(stream);
    prompt.log_in(ALICE).await;
    // 500 ms before the handshake and 500 ms after it: 1000 ms from the accept is over 900, though neither half is.
    let socket = TcpStream::connect(server.local_addr()).await.unwrap();
    sleep(Duration::from_millis(500)).await;
    let stream = connector("root.pem").connect(name, socket).await.unwrap();
    let mut slow = Client::over(stream);
    sleep(Duration::from_millis(500)).await;
    slow.send(ApiKey::SaslHandshake, 1, &sasl_handshake()).await;
    assert!(
        slow.is_closed().await,
        "the login came after the budget that began at the accept"
    );
    server.shutdown().await;
}

fn sasl_handshake() -> kafka_wire::messages::SaslHandshakeRequest {
    let mut r = kafka_wire::messages::SaslHandshakeRequest::default();
    r.mechanism = StrBytes::from_static_str("PLAIN");
    r
}

fn allow(topic: &str, host: &str, deny: bool) -> AclCreation {
    let mut c = AclCreation::default();
    c.resource_type = 2;
    c.resource_name = StrBytes::from_string(topic.to_owned());
    c.resource_pattern_type = 3;
    c.principal = StrBytes::from_static_str("User:ANONYMOUS");
    c.host = StrBytes::from_string(host.to_owned());
    c.operation = 8;
    c.permission_type = if deny { 2 } else { 3 };
    c
}

#[tokio::test]
async fn an_acl_by_host_sees_the_peers_address_over_tls() {
    let server = start(Opts::default()).await;
    let mut c = dial(&server).await;
    let mut req = CreateAclsRequest::default();
    req.creations = vec![
        allow("here", "127.0.0.1", false),
        allow("elsewhere", "192.0.2.77", false),
        allow("blocked", "127.0.0.1", true),
        allow("blocked", "*", false),
    ];
    let made: CreateAclsResponse = c.call(ApiKey::CreateAcls, 3, &req).await.unwrap();
    assert!(made.results.iter().all(|r| r.error_code == 0), "{made:?}");
    let mut error = async |topic: &str| {
        let r: MetadataResponse = c
            .call(ApiKey::Metadata, 9, &metadata_for(topic))
            .await
            .unwrap();
        r.topics[0].error_code
    };
    assert_ne!(
        error("here").await,
        29,
        "an ACL for 127.0.0.1 matches a TLS peer at 127.0.0.1"
    );
    assert_eq!(
        error("elsewhere").await,
        29,
        "an ACL for another address does not"
    );
    assert_eq!(
        error("blocked").await,
        29,
        "a deny for 127.0.0.1 wins over an allow for all"
    );
    server.shutdown().await;
}

#[tokio::test]
async fn a_stop_ends_tls_connections_with_close_notify_and_does_not_wait_for_a_stalled_handshake() {
    let server = start(Opts::default()).await;
    let mut open = Vec::new();
    for _ in 0..3 {
        let mut c = dial(&server).await;
        assert!(c.versions().await.is_some());
        open.push(c);
    }
    // One mid-handshake (a TCP connection that has said nothing) and one idle in the middle of a TLS record.
    let stalled = TcpStream::connect(server.local_addr()).await.unwrap();
    sleep(Duration::from_millis(100)).await;
    let began = Instant::now();
    timeout(Duration::from_secs(5), server.shutdown())
        .await
        .expect("the stop was not held up");
    assert!(
        began.elapsed() < Duration::from_secs(3),
        "{:?}",
        began.elapsed()
    );
    for mut c in open {
        let mut byte = [0u8; 1];
        // Ok(0) is a clean end: the close_notify arrived. A cut connection would be an error (an unexpected EOF).
        let n = timeout(Duration::from_secs(2), c.socket.read(&mut byte))
            .await
            .unwrap();
        assert_eq!(n.unwrap(), 0, "the server said close_notify");
    }
    drop(stalled);
}
