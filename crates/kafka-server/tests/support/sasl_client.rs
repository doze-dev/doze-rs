//! A raw client for a broker with SASL (PLAIN and SCRAM) on a real socket, shared by the SASL tests.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use kafka_auth::scram::Kind;
use kafka_auth::scram::client::Client as ScramClient;
use kafka_auth::{Authenticator, Credentials, Mechanism};
use kafka_broker::{Broker, Config as BrokerConfig};
use kafka_protocol::protocol::{Decodable, Encodable, StrBytes};
use kafka_server::{Config, Running, Server};
use kafka_storage::sim::SimFs;
use kafka_wire::messages::metadata_request::MetadataRequestTopic;
use kafka_wire::messages::{
    ApiKey, ApiVersionsRequest, ApiVersionsResponse, MetadataRequest, ResponseHeader,
    SaslAuthenticateRequest, SaslAuthenticateResponse, SaslHandshakeRequest, SaslHandshakeResponse,
    TopicName,
};
use kafka_wire::testing::{framed, request_payload};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

pub(crate) const SECRET: &str = "alice-secret-planted-7f3a";

pub(crate) const BOB_SECRET: &str = "bob-secret-planted-91c2";

/// PLAIN alone, with or without SASL on.
pub(crate) async fn start_with(sasl: bool) -> Running {
    start_offering(&[Mechanism::Plain], sasl).await
}

/// Every mechanism, the way the conformance harness starts the broker for `sasl-scram`.
pub(crate) async fn start_scram() -> Running {
    let all = [
        Mechanism::Plain,
        Mechanism::ScramSha512,
        Mechanism::ScramSha256,
    ];
    start_offering(&all, true).await
}

async fn start_offering(mechanisms: &[Mechanism], sasl: bool) -> Running {
    let users = [("alice", SECRET), ("bob", BOB_SECRET)].map(|(u, p)| (u.to_owned(), p.to_owned()));
    let auth = Authenticator::new(mechanisms, Credentials::new(users).unwrap()).unwrap();
    let config = Config {
        listen: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        sasl: sasl.then(|| Arc::new(auth)),
        ..Config::default()
    };
    let broker = Broker::open(SimFs::new(), BrokerConfig::new("/data".into())).unwrap();
    Server::bind(config, Arc::new(broker)).await.unwrap()
}

/// A raw client: what a library would send, one request at a time.
pub(crate) struct Client {
    pub(crate) socket: TcpStream,
    pub(crate) id: i32,
}

impl Client {
    pub(crate) async fn connect(server: &Running) -> Self {
        let socket = TcpStream::connect(server.local_addr()).await.unwrap();
        Self { socket, id: 0 }
    }

    pub(crate) async fn send<R: Encodable>(&mut self, key: ApiKey, version: i16, body: &R) {
        self.id += 1;
        let request = framed(&request_payload(key, version, self.id, Some("t"), body));
        // A write to a connection the server has closed may fail; the read says what happened.
        let _ = self.socket.write_all(&request).await;
    }

    /// The next frame, or `None` if the server closed the connection instead.
    pub(crate) async fn read(&mut self) -> Option<Bytes> {
        let mut len = [0; 4];
        let waited = timeout(Duration::from_secs(5), self.socket.read_exact(&mut len)).await;
        waited.expect("the server answered or closed").ok()?;
        let mut payload = vec![0; u32::from_be_bytes(len) as usize];
        self.socket.read_exact(&mut payload).await.ok()?;
        Some(Bytes::from(payload))
    }

    pub(crate) async fn call<R: Encodable, T: Decodable>(
        &mut self,
        key: ApiKey,
        version: i16,
        body: &R,
    ) -> Option<T> {
        self.send(key, version, body).await;
        let mut payload = self.read().await?;
        let header = ResponseHeader::decode(&mut payload, key.response_header_version(version));
        assert_eq!(header.unwrap().correlation_id, self.id);
        Some(T::decode(&mut payload, version).unwrap())
    }

    pub(crate) async fn handshake(
        &mut self,
        mechanism: &'static str,
    ) -> Option<SaslHandshakeResponse> {
        let mut r = SaslHandshakeRequest::default();
        r.mechanism = StrBytes::from_static_str(mechanism);
        self.call(ApiKey::SaslHandshake, 1, &r).await
    }

    pub(crate) async fn token(&mut self, bytes: &[u8]) -> Option<SaslAuthenticateResponse> {
        let mut r = SaslAuthenticateRequest::default();
        r.auth_bytes = Bytes::copy_from_slice(bytes);
        self.call(ApiKey::SaslAuthenticate, 2, &r).await
    }

    /// Handshake and token, expecting both to succeed.
    pub(crate) async fn log_in(&mut self, token: &[u8]) {
        assert_eq!(self.handshake("PLAIN").await.unwrap().error_code, 0);
        let done = self.token(token).await.unwrap();
        assert_eq!(
            (done.error_code, done.error_message.as_deref()),
            (0, Some(""))
        );
    }

    /// The handshake for `kind` and the first SCRAM token of `client`: the answer to the token.
    pub(crate) async fn scram_first(
        &mut self,
        kind: Kind,
        client: &ScramClient,
    ) -> SaslAuthenticateResponse {
        let hs = self.handshake(kind.mechanism().name()).await.unwrap();
        assert_eq!(hs.error_code, 0);
        self.token(&client.first()).await.unwrap()
    }

    /// A whole SCRAM login that is expected to succeed, signature checked.
    pub(crate) async fn scram_log_in(&mut self, kind: Kind, user: &str, password: &str) {
        let client = ScramClient::new(kind, user, password, "client-nonce-0001.");
        let challenge = self.scram_first(kind, &client).await;
        assert_eq!(challenge.error_code, 0, "{challenge:?}");
        let last = client.last(&challenge.auth_bytes, None, "");
        let done = self.token(&last.message).await.unwrap();
        assert_eq!(
            (done.error_code, done.error_message.as_deref()),
            (0, Some(""))
        );
        assert_eq!(done.auth_bytes.as_ref(), last.expected.as_slice());
    }

    pub(crate) async fn versions(&mut self) -> Option<ApiVersionsResponse> {
        self.call(ApiKey::ApiVersions, 3, &versions_request()).await
    }

    pub(crate) async fn is_closed(&mut self) -> bool {
        self.read().await.is_none()
    }
}

pub(crate) fn versions_request() -> ApiVersionsRequest {
    let mut body = ApiVersionsRequest::default();
    body.client_software_name = StrBytes::from_static_str("test");
    body.client_software_version = StrBytes::from_static_str("1");
    body
}

pub(crate) fn metadata_for(topic: &str) -> MetadataRequest {
    let mut t = MetadataRequestTopic::default();
    t.name = Some(TopicName(StrBytes::from_string(topic.to_owned())));
    let mut req = MetadataRequest::default();
    req.topics = Some(vec![t]);
    req.allow_auto_topic_creation = true;
    req
}

pub(crate) const ALICE: &[u8] = b"\0alice\0alice-secret-planted-7f3a";
