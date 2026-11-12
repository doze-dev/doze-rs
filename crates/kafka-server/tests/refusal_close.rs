//! A refused login on a real socket, with more bytes behind it than the server has read (SEC-195).
//!
//! Closing a socket that still holds unread bytes sends a reset, and a reset can take the answer the client has not yet
//! read with it. So after the answer the server shuts its side down (the client reads the answer and then an end), and
//! reads and throws away what the client sent behind the refusal, so that when it closes there is nothing unread.
//! In-memory streams cannot show this: they have no receive buffer to be left unread.
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
use std::time::{Duration, Instant};

use bytes::Bytes;
use client::{Client, metadata_for};
use kafka_auth::{Authenticator, Credentials, Mechanism};
use kafka_broker::{Broker, Config as BrokerConfig};
use kafka_protocol::protocol::{Decodable, Encodable, StrBytes};
use kafka_server::{Config, Running, Server, Timeouts};
use kafka_storage::sim::SimFs;
use kafka_wire::messages::{
    ApiKey, ResponseHeader, SaslAuthenticateRequest, SaslAuthenticateResponse, SaslHandshakeRequest,
};
use kafka_wire::testing::{framed, request_payload};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

async fn start() -> Running {
    let users = [("alice".to_owned(), "alice-secret".to_owned())];
    let auth = Authenticator::new(&[Mechanism::Plain], Credentials::new(users).unwrap()).unwrap();
    let config = Config {
        listen: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        sasl: Some(Arc::new(auth)),
        // The delay is not what is tried here.
        timeouts: Timeouts {
            failed_login: Duration::ZERO,
            ..Timeouts::default()
        },
        ..Config::default()
    };
    let broker = Broker::open(SimFs::new(), BrokerConfig::new("/data".into())).unwrap();
    Server::bind(config, Arc::new(broker)).await.unwrap()
}

fn frame<R: Encodable>(key: ApiKey, version: i16, id: i32, body: &R) -> Vec<u8> {
    framed(&request_payload(key, version, id, Some("t"), body)).to_vec()
}

/// A handshake, a wrong password and then `junk` Metadata requests, all in one write.
fn refused_login_with_requests_behind_it(junk: usize) -> Vec<u8> {
    let mut hs = SaslHandshakeRequest::default();
    hs.mechanism = StrBytes::from_static_str("PLAIN");
    let mut auth = SaslAuthenticateRequest::default();
    auth.auth_bytes = Bytes::from_static(b"\0alice\0wrong");
    let mut all = frame(ApiKey::SaslHandshake, 1, 1, &hs);
    all.extend(frame(ApiKey::SaslAuthenticate, 2, 2, &auth));
    for i in 0..junk {
        let id = 100 + i32::try_from(i).unwrap();
        all.extend(frame(ApiKey::Metadata, 12, id, &metadata_for("t")));
    }
    all
}

#[tokio::test]
async fn a_refused_login_with_bytes_behind_it_is_answered_in_full_and_then_ends_cleanly() {
    let server = start().await;
    for round in 0..40 {
        let mut c = Client::connect(&server).await;
        // Far more than the server reads before it answers: the rest is in its receive buffer when it closes.
        let all = refused_login_with_requests_behind_it(4000);
        c.socket.write_all(&all).await.unwrap();
        let _handshake = c.read().await.expect("the handshake is answered");
        let mut answer = c.read().await.expect("the refusal is answered in full");
        ResponseHeader::decode(&mut answer, 1).unwrap();
        let said = SaslAuthenticateResponse::decode(&mut answer, 2).unwrap();
        assert_eq!(said.error_code, 58, "round {round}");
        // Then an end, not a reset, and promptly: the server shut its side down as soon as it had answered.
        let waited = Instant::now();
        let mut rest = Vec::new();
        let ended = tokio::time::timeout(Duration::from_secs(5), c.socket.read_to_end(&mut rest))
            .await
            .expect("the connection ended");
        ended.unwrap_or_else(|e| panic!("round {round}: a reset, not an end: {e}"));
        assert!(rest.is_empty(), "round {round}: {} stray bytes", rest.len());
        assert!(
            waited.elapsed() < Duration::from_millis(150),
            "round {round}: the end took {:?}",
            waited.elapsed()
        );
    }
    server.shutdown().await;
}
