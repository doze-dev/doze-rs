//! TLS on a real socket (feature `tls`): the handshake comes first and a Kafka client then works as on any other
//! listener; a stranger, a plaintext client and a broken handshake are closed without a word of Kafka; the peer's
//! address is the TCP socket's; the handshake is inside the pre-auth budget; a stop says close_notify.
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

use std::sync::Arc;
use std::time::{Duration, Instant};

use client::{ALICE, Client, SECRET, metadata_for};
use kafka_auth::scram::Kind;
use kafka_protocol::protocol::StrBytes;
use kafka_wire::batch::build::BatchBuilder;
use kafka_wire::messages::create_acls_request::AclCreation;
use kafka_wire::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::{
    ApiKey, CreateAclsRequest, CreateAclsResponse, FetchRequest, FetchResponse, MetadataResponse,
    ProduceRequest, ProduceResponse, TopicName,
};
use rustls::pki_types::ServerName;
use rustls::pki_types::pem::PemObject;
use support::{Opts, connector, dial, fixture, handshake, start};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};

/// Makes topic `t`, writes three records to it and reads them back.
async fn produce_and_fetch<S: AsyncRead + AsyncWrite + Unpin>(c: &mut Client<S>) {
    for _ in 0..2 {
        c.call::<_, MetadataResponse>(ApiKey::Metadata, 12, &metadata_for("t")).await.unwrap();
    }
    let mut partition = PartitionProduceData::default();
    partition.index = 0;
    partition.records = Some(BatchBuilder::new().records(3).build());
    let mut topic = TopicProduceData::default();
    topic.name = TopicName(StrBytes::from_static_str("t"));
    topic.partition_data.push(partition);
    let mut produce = ProduceRequest::default();
    produce.acks = 1;
    produce.timeout_ms = 5000;
    produce.topic_data.push(topic);
    let wrote: ProduceResponse = c.call(ApiKey::Produce, 9, &produce).await.unwrap();
    assert_eq!(wrote.responses[0].partition_responses[0].error_code, 0);

    let mut fp = FetchPartition::default();
    fp.partition = 0;
    fp.partition_max_bytes = 1 << 20;
    let mut ft = FetchTopic::default();
    ft.topic = TopicName(StrBytes::from_static_str("t"));
    ft.partitions.push(fp);
    let mut fetch = FetchRequest::default();
    fetch.max_bytes = 1 << 20;
    fetch.topics.push(ft);
    let read: FetchResponse = c.call(ApiKey::Fetch, 12, &fetch).await.unwrap();
    let records = read.responses[0].partitions[0].records.as_ref();
    assert!(records.is_some_and(|r| !r.is_empty()), "the records written come back");
}

/// Everything the server sends to `socket` until it closes it (or `wait` runs out).
async fn drain(socket: &mut TcpStream, wait: Duration) -> Vec<u8> {
    let mut all = Vec::new();
    let _ = timeout(wait, socket.read_to_end(&mut all)).await.expect("the server closed the connection");
    all
}

#[tokio::test]
async fn a_client_that_handshakes_can_produce_and_fetch() {
    let server = start(Opts::default()).await;
    let mut c = dial(&server).await;
    produce_and_fetch(&mut c).await;
    server.shutdown().await;
}

#[tokio::test]
async fn every_key_format_serves_and_the_chain_is_sent_whole() {
    for (cert, key) in [
        ("ec-chain.pem", "ec-key-pkcs8.pem"),
        ("ec-chain.pem", "ec-key-sec1.pem"),
        ("rsa-chain.pem", "rsa-key-pkcs8.pem"),
        ("rsa-chain.pem", "rsa-key-pkcs1.pem"),
    ] {
        let server = start(Opts { cert, key, ..Opts::default() }).await;
        let mut c = dial(&server).await;
        assert!(c.versions().await.is_some(), "{cert} with {key}");
        server.shutdown().await;
    }
    // A certificate without the intermediate above it cannot be verified from the root alone: the chain in the file is
    // what the server sends, no more.
    let server = start(Opts { cert: "ec-leaf-only.pem", key: "ec-key-sec1.pem", ..Opts::default() }).await;
    assert!(handshake(&server, &connector("root.pem")).await.is_err());
    server.shutdown().await;
}

#[tokio::test]
async fn tls_one_two_and_one_three_are_both_offered_and_the_address_may_be_an_ip() {
    use rustls::ClientConfig;
    use rustls::crypto::ring::default_provider;
    use rustls::version::{TLS12, TLS13};
    let server = start(Opts::default()).await;
    for (version, name) in [(&TLS12, "TLSv1_2"), (&TLS13, "TLSv1_3")] {
        let mut roots = rustls::RootCertStore::empty();
        for der in rustls::pki_types::CertificateDer::pem_file_iter(fixture("root.pem")).unwrap() {
            roots.add(der.unwrap()).unwrap();
        }
        let config = ClientConfig::builder_with_provider(Arc::new(default_provider()))
            .with_protocol_versions(&[version])
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let socket = TcpStream::connect(server.local_addr()).await.unwrap();
        let ip = ServerName::IpAddress(std::net::Ipv4Addr::LOCALHOST.into());
        let stream = tokio_rustls::TlsConnector::from(Arc::new(config)).connect(ip, socket).await.unwrap();
        assert_eq!(format!("{:?}", stream.get_ref().1.protocol_version().unwrap()), name);
        assert!(Client::over(stream).versions().await.is_some());
    }
    server.shutdown().await;
}

#[tokio::test]
async fn sasl_and_tls_are_independent_and_both_work_together() {
    let server = start(Opts { sasl: true, ..Opts::default() }).await;
    let mut plain = dial(&server).await;
    plain.log_in(ALICE).await;
    produce_and_fetch(&mut plain).await;
    for (kind, user, password) in [(Kind::Sha256, "alice", SECRET), (Kind::Sha512, "bob", client::BOB_SECRET)] {
        let mut c = dial(&server).await;
        c.scram_log_in(kind, user, password).await;
        assert!(c.versions().await.is_some());
    }
    // A wrong password over TLS is answered and closed, as over any socket.
    let mut wrong = dial(&server).await;
    assert_eq!(wrong.handshake("PLAIN").await.unwrap().error_code, 0);
    assert_eq!(wrong.token(b"\0alice\0wrong-planted").await.unwrap().error_code, 58);
    assert!(wrong.is_closed().await);
    // And a stranger who handshakes but does not log in is served nothing.
    let mut stranger = dial(&server).await;
    stranger.send(ApiKey::Metadata, 12, &metadata_for("t")).await;
    assert!(stranger.is_closed().await);
    server.shutdown().await;
}

#[tokio::test]
async fn a_plaintext_client_on_the_tls_port_is_closed_and_gets_no_kafka_answer() {
    let server = start(Opts::default()).await;
    let mut socket = TcpStream::connect(server.local_addr()).await.unwrap();
    let request = kafka_wire::testing::framed(&kafka_wire::testing::request_payload(
        ApiKey::ApiVersions,
        3,
        1,
        Some("t"),
        &client::versions_request(),
    ));
    socket.write_all(&request).await.unwrap();
    let said = drain(&mut socket, Duration::from_secs(5)).await;
    // At most a TLS alert (a few bytes, content type 21), never a response frame.
    assert!(said.len() <= 8, "{said:?}");
    assert!(said.first().is_none_or(|b| *b == 21), "{said:?}");
    // HTTP, too.
    let mut http = TcpStream::connect(server.local_addr()).await.unwrap();
    http.write_all(b"GET / HTTP/1.1\r\nHost: planted\r\n\r\n").await.unwrap();
    assert!(drain(&mut http, Duration::from_secs(5)).await.len() <= 8);
    server.shutdown().await;
}

#[tokio::test]
async fn a_broken_handshake_closes_quietly_and_the_server_goes_on() {
    let server = start(Opts::default()).await;
    // Half a ClientHello, then silence closed by the client.
    let mut conn = rustls::ClientConnection::new(
        connector("root.pem").config().clone(),
        ServerName::try_from("localhost").unwrap(),
    )
    .unwrap();
    let mut hello = Vec::new();
    conn.write_tls(&mut hello).unwrap();
    let mut half = TcpStream::connect(server.local_addr()).await.unwrap();
    half.write_all(&hello[..hello.len() / 2]).await.unwrap();
    half.shutdown().await.unwrap();
    assert!(drain(&mut half, Duration::from_secs(5)).await.is_empty());
    // A client that does not trust the certificate hangs up on the server in the middle of the handshake.
    assert!(handshake(&server, &connector("other-root.pem")).await.is_err());
    // A TLS record of a kind that does not exist.
    let mut junk = TcpStream::connect(server.local_addr()).await.unwrap();
    junk.write_all(&[0x7f, 3, 3, 0, 4, 1, 2, 3, 4]).await.unwrap();
    assert!(drain(&mut junk, Duration::from_secs(5)).await.len() <= 8);
    // None of it hurt: a good client is served.
    let mut c = dial(&server).await;
    produce_and_fetch(&mut c).await;
    server.shutdown().await;
}

#[tokio::test]
async fn a_connection_that_never_handshakes_is_cut_by_the_pre_auth_budget_and_does_not_block_others() {
    let server = start(Opts { pre_auth: Some(Duration::from_millis(400)), ..Opts::default() }).await;
    let began = Instant::now();
    let mut silent = TcpStream::connect(server.local_addr()).await.unwrap();
    // While it sits there the accept loop and the other connections are free.
    let mut c = dial(&server).await;
    assert!(c.versions().await.is_some());
    let said = drain(&mut silent, Duration::from_secs(5)).await;
    assert!(said.is_empty(), "nothing is written to a connection that never began");
    let took = began.elapsed();
    assert!(took >= Duration::from_millis(350) && took < Duration::from_secs(3), "{took:?}");
    server.shutdown().await;
}

#[tokio::test]
async fn the_handshake_counts_against_the_login_budget_it_does_not_extend_it() {
    let pre_auth = Duration::from_millis(900);
    let server = start(Opts { sasl: true, pre_auth: Some(pre_auth), ..Opts::default() }).await;
    // 500 ms before the handshake and a prompt login: inside the budget.
    let socket = TcpStream::connect(server.local_addr()).await.unwrap();
    sleep(Duration::from_millis(500)).await;
    let name = ServerName::try_from("localhost").unwrap();
    let stream = connector("root.pem").connect(name.clone(), socket).await.unwrap();
    let mut prompt = Client::over(stream);
    prompt.log_in(ALICE).await;
    // 500 ms before the handshake and 500 ms after it: 1000 ms from the accept is over 900, though neither half is.
    let socket = TcpStream::connect(server.local_addr()).await.unwrap();
    sleep(Duration::from_millis(500)).await;
    let stream = connector("root.pem").connect(name, socket).await.unwrap();
    let mut slow = Client::over(stream);
    sleep(Duration::from_millis(500)).await;
    slow.send(ApiKey::SaslHandshake, 1, &sasl_handshake()).await;
    assert!(slow.is_closed().await, "the login came after the budget that began at the accept");
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
        let r: MetadataResponse = c.call(ApiKey::Metadata, 9, &metadata_for(topic)).await.unwrap();
        r.topics[0].error_code
    };
    assert_ne!(error("here").await, 29, "an ACL for 127.0.0.1 matches a TLS peer at 127.0.0.1");
    assert_eq!(error("elsewhere").await, 29, "an ACL for another address does not");
    assert_eq!(error("blocked").await, 29, "a deny for 127.0.0.1 wins over an allow for all");
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
    timeout(Duration::from_secs(5), server.shutdown()).await.expect("the stop was not held up");
    assert!(began.elapsed() < Duration::from_secs(3), "{:?}", began.elapsed());
    for mut c in open {
        let mut byte = [0u8; 1];
        // Ok(0) is a clean end: the close_notify arrived. A cut connection would be an error (an unexpected EOF).
        let n = timeout(Duration::from_secs(2), c.socket.read(&mut byte)).await.unwrap();
        assert_eq!(n.unwrap(), 0, "the server said close_notify");
    }
    drop(stalled);
}
