//! A local HTTPS server for tests (feature `test-server`, never in a release build): a CA made once per process, a server
//! certificate for `localhost` and `127.0.0.1` signed by it (or self-signed, which nothing trusts), and a server that keeps every
//! request and answers what the test says.
//!
//! [`Ca::install`] makes the process's client trust that CA alone (not the machine's store), as `--ca-file` would.

// WHY: test support; a failure to make a certificate or bind a port is the test's failure.
#![allow(clippy::expect_used, clippy::missing_panics_doc)]

use std::sync::{Arc, Mutex, OnceLock};

use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use crate::Trust;

/// The names every test certificate is for.
const NAMES: [&str; 2] = ["localhost", "127.0.0.1"];

/// A certificate authority.
#[derive(Debug)]
pub struct Ca {
    issuer: CertifiedIssuer<'static, KeyPair>,
}

/// A server's certificate and key.
#[derive(Debug)]
pub struct Identity {
    chain: Vec<CertificateDer<'static>>,
    key: Vec<u8>,
    pem: String,
}

impl Ca {
    /// The process's CA (made on first use).
    pub fn get() -> &'static Self {
        static CA: OnceLock<Ca> = OnceLock::new();
        CA.get_or_init(|| {
            let mut params = CertificateParams::new(Vec::<String>::new()).expect("CA params");
            params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
            params
                .distinguished_name
                .push(rcgen::DnType::CommonName, "doze test CA");
            let key = KeyPair::generate().expect("CA key");
            Self {
                issuer: CertifiedIssuer::self_signed(params, key).expect("CA certificate"),
            }
        })
    }

    /// The CA certificate as PEM (what a `--ca-file` holds).
    #[must_use]
    pub fn pem(&self) -> String {
        self.issuer.pem()
    }

    /// Trust in this CA alone.
    #[must_use]
    pub fn trust(&self) -> Trust {
        Trust {
            os_store: false,
            extra: vec![self.issuer.der().clone()],
            insecure: false,
        }
    }

    /// Makes the process's client trust this CA (the first configuration wins; every test of a binary shares the one CA, and an
    /// unconfigured client in a build with this feature trusts it anyway).
    pub fn install(&self) {
        crate::configure(self.trust());
    }

    /// A server certificate for `localhost` and `127.0.0.1`, signed by this CA.
    #[must_use]
    pub fn identity(&self) -> Identity {
        let key = KeyPair::generate().expect("server key");
        let cert = params()
            .signed_by(&key, &self.issuer)
            .expect("server certificate");
        Identity {
            chain: vec![cert.der().clone()],
            key: key.serialize_der(),
            pem: cert.pem(),
        }
    }
}

fn params() -> CertificateParams {
    CertificateParams::new(NAMES.map(str::to_owned).to_vec()).expect("server params")
}

impl Identity {
    /// A self-signed certificate for `localhost` and `127.0.0.1`: what a developer's local server often has.
    #[must_use]
    pub fn self_signed() -> Self {
        let key = KeyPair::generate().expect("server key");
        let cert = params().self_signed(&key).expect("self-signed certificate");
        Self {
            chain: vec![cert.der().clone()],
            key: key.serialize_der(),
            pem: cert.pem(),
        }
    }

    /// The certificate as PEM (a self-signed certificate is its own `--ca-file`).
    #[must_use]
    pub fn pem(&self) -> &str {
        &self.pem
    }
}

/// What the server answers a request (given its raw text): the raw HTTP answer.
pub type Answerer = Arc<dyn Fn(&str) -> String + Send + Sync>;

/// A running server.
#[derive(Debug)]
pub struct Server {
    pub port: u16,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Server {
    /// `https://127.0.0.1:<port>`.
    #[must_use]
    pub fn url(&self) -> String {
        format!("https://127.0.0.1:{}", self.port)
    }

    /// The requests received so far (raw text), waiting up to three seconds for at least `n`.
    pub async fn seen(&self, n: usize) -> Vec<String> {
        for _ in 0..300 {
            if self.seen.lock().map(|s| s.len()).unwrap_or(0) >= n {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        self.seen.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

/// Answers 200 with an empty body.
#[must_use]
pub fn ok() -> Answerer {
    Arc::new(|_| "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned())
}

/// Starts an HTTPS server on `127.0.0.1` with `identity`, answering with `answer`.
pub async fn serve(identity: Identity, answer: Answerer) -> Server {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key));
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .with_no_client_auth()
        .with_single_cert(identity.chain, key)
        .expect("server config");
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("address").port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let (acceptor, log, answer) = (acceptor.clone(), Arc::clone(&log), Arc::clone(&answer));
            tokio::spawn(async move {
                // A refused handshake (the client did not trust us) is the test's point, not an error here.
                let Ok(mut s) = acceptor.accept(tcp).await else {
                    return;
                };
                let request = read_request(&mut s).await;
                let reply = answer(&request);
                if let Ok(mut l) = log.lock() {
                    l.push(request);
                }
                s.write_all(reply.as_bytes()).await.ok();
                s.shutdown().await.ok();
            });
        }
    });
    Server { port, seen }
}

/// One request: its head and the body its length promises.
async fn read_request<S: tokio::io::AsyncRead + Unpin>(s: &mut S) -> String {
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = s.read(&mut buf).await.unwrap_or(0);
        raw.extend_from_slice(buf.get(..n).unwrap_or_default());
        let text = String::from_utf8_lossy(&raw).to_string();
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let want = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(str::to_owned)
                })
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            if body.len() >= want {
                return text;
            }
        }
        if n == 0 {
            return text;
        }
    }
}
