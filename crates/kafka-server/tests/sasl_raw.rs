//! SaslHandshake v0 (SEC-401): after it the mechanism's tokens are raw, size-prefixed frames with no Kafka header, and
//! the server's replies are raw too, as Apache Kafka 4.3.1 serves it (`sasl-handshake-v0` in tools/kafka-replay). A
//! refused raw token is answered with nothing: the connection closes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/sasl_client.rs"]
mod client;

use client::{ALICE, Client, SECRET, metadata_for, start_scram, start_with};
use kafka_auth::scram::Kind;
use kafka_auth::scram::client::Client as ScramClient;
use kafka_wire::messages::{ApiKey, MetadataResponse};

#[tokio::test]
async fn plain_over_raw_tokens_logs_in_with_an_empty_reply() {
    let server = start_with(true).await;
    let mut c = Client::connect(&server).await;
    let hs = c.handshake_v0("PLAIN").await.unwrap();
    assert_eq!(hs.error_code, 0);
    assert_eq!(hs.mechanisms.len(), 1);
    // PLAIN's reply is empty, and Kafka writes it: a frame of length 0.
    let reply = c.raw_token(ALICE).await.expect("a raw reply");
    assert!(reply.is_empty());
    // Logged in: requests are Kafka requests again.
    let made: Option<MetadataResponse> = c.call(ApiKey::Metadata, 12, &metadata_for("t")).await;
    assert!(made.is_some());
    server.shutdown().await;
}

#[tokio::test]
async fn a_wrong_raw_token_is_answered_with_nothing_and_closed() {
    let server = start_with(true).await;
    for token in [&b"\0alice\0nope"[..], b"not a plain token", b""] {
        let mut c = Client::connect(&server).await;
        assert_eq!(c.handshake_v0("PLAIN").await.unwrap().error_code, 0);
        assert!(c.raw_token(token).await.is_none(), "{token:?}");
    }
    server.shutdown().await;
}

#[tokio::test]
async fn a_kafka_request_after_a_v0_handshake_is_read_as_a_raw_token() {
    let server = start_with(true).await;
    let mut c = Client::connect(&server).await;
    assert_eq!(c.handshake_v0("PLAIN").await.unwrap().error_code, 0);
    // A SaslAuthenticate request with a right token inside is not a PLAIN token: refused, nothing written.
    assert!(c.token(ALICE).await.is_none());
    server.shutdown().await;
}

#[tokio::test]
async fn scram_over_raw_tokens_logs_in_with_the_server_signature() {
    let server = start_scram().await;
    for kind in [Kind::Sha256, Kind::Sha512] {
        let mut c = Client::connect(&server).await;
        let hs = c.handshake_v0(kind.mechanism().name()).await.unwrap();
        assert_eq!(hs.error_code, 0);
        let client = ScramClient::new(kind, "alice", SECRET, "client-nonce-0001.");
        let challenge = c.raw_token(&client.first()).await.expect("server-first");
        let last = client.last(&challenge, None, "");
        let signature = c.raw_token(&last.message).await.expect("server-final");
        assert_eq!(signature.as_ref(), last.expected.as_slice());
        let made: Option<MetadataResponse> = c.call(ApiKey::Metadata, 12, &metadata_for("t")).await;
        assert!(made.is_some(), "{kind:?}");
    }
    // A wrong password: the final raw token is refused with nothing written.
    let mut c = Client::connect(&server).await;
    assert_eq!(c.handshake_v0("SCRAM-SHA-256").await.unwrap().error_code, 0);
    let client = ScramClient::new(Kind::Sha256, "alice", "wrong", "client-nonce-0002.");
    let challenge = c.raw_token(&client.first()).await.expect("server-first");
    let last = client.last(&challenge, None, "");
    assert!(c.raw_token(&last.message).await.is_none());
    server.shutdown().await;
}

#[tokio::test]
async fn a_v1_handshake_still_wants_saslauthenticate_requests() {
    let server = start_with(true).await;
    let mut c = Client::connect(&server).await;
    c.log_in(ALICE).await;
    let made: Option<MetadataResponse> = c.call(ApiKey::Metadata, 12, &metadata_for("t")).await;
    assert!(made.is_some());
    server.shutdown().await;
}
