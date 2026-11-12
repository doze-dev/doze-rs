//! Starting a broker with TLS and dialling it with a rustls client, shared by the TLS tests. The certificates are
//! the committed ones of `tls-fixtures/` (its README says how they were made).
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use kafka_auth::{Authenticator, Credentials, Mechanism};
use kafka_broker::{Broker, Config as BrokerConfig};
use kafka_server::{Config, Running, Server, TlsConfig};
use kafka_storage::sim::SimFs;
use rustls::ClientConfig;
use rustls::RootCertStore;
use rustls::crypto::ring::default_provider;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use crate::client::Client;

pub(crate) fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/tls-fixtures")
        .join(name)
}

/// How a test wants the broker.
pub(crate) struct Opts {
    pub(crate) cert: &'static str,
    pub(crate) key: &'static str,
    /// Every mechanism, with the users alice and bob; none means no SASL.
    pub(crate) sasl: bool,
    pub(crate) acl_default_allow: bool,
    pub(crate) pre_auth: Option<Duration>,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            cert: "ec-chain.pem",
            key: "ec-key-pkcs8.pem",
            sasl: false,
            acl_default_allow: true,
            pre_auth: None,
        }
    }
}

pub(crate) async fn start(opts: Opts) -> Running {
    let users = [
        ("alice", crate::client::SECRET),
        ("bob", crate::client::BOB_SECRET),
    ]
    .map(|(u, p)| (u.to_owned(), p.to_owned()));
    let mechanisms = [
        Mechanism::Plain,
        Mechanism::ScramSha512,
        Mechanism::ScramSha256,
    ];
    let auth = Authenticator::new(&mechanisms, Credentials::new(users).unwrap()).unwrap();
    let mut config = Config {
        listen: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        sasl: opts.sasl.then(|| Arc::new(auth)),
        tls: Some(TlsConfig::load(fixture(opts.cert), fixture(opts.key)).unwrap()),
        ..Config::default()
    };
    if let Some(pre_auth) = opts.pre_auth {
        config.timeouts.pre_auth = pre_auth;
    }
    let mut broker = BrokerConfig::new("/data".into());
    broker.acl_default_allow = opts.acl_default_allow;
    let broker = Broker::open(SimFs::new(), broker).unwrap();
    Server::bind(config, Arc::new(broker)).await.unwrap()
}

/// A client that trusts the root in `trust` (a file of `tls-fixtures/`) and offers `versions` of TLS.
pub(crate) fn connector(trust: &str) -> TlsConnector {
    let mut roots = RootCertStore::empty();
    for der in CertificateDer::pem_file_iter(fixture(trust)).unwrap() {
        roots.add(der.unwrap()).unwrap();
    }
    let config = ClientConfig::builder_with_provider(Arc::new(default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

/// Connects and handshakes as `localhost`.
pub(crate) async fn handshake(
    server: &Running,
    connector: &TlsConnector,
) -> io::Result<TlsStream<TcpStream>> {
    let socket = TcpStream::connect(server.local_addr()).await?;
    connector
        .connect(ServerName::try_from("localhost").unwrap(), socket)
        .await
}

/// A Kafka client over a good TLS connection.
pub(crate) async fn dial(server: &Running) -> Client<TlsStream<TcpStream>> {
    let stream = handshake(server, &connector("root.pem")).await.unwrap();
    Client::over(stream)
}
