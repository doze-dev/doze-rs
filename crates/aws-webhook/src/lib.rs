//! The HTTP requests the services make to the outside: an SNS `http`/`https` subscription, an EventBridge API destination, an API
//! Gateway HTTP integration, a JWT authorizer's key fetch. HTTP/1.1, one request per connection (`Connection: close`; nothing here
//! kept connections before, and none of these callers sends often enough to the same place for a pool to pay for its idle sockets),
//! over plain TCP for `http://` and TLS (rustls) for `https://`.
//!
//! **Trust** ([`Trust`], `tls.rs`) is set once at start ([`configure`]) from doze-aws's flags; the TLS configuration is built on the
//! first `https://` call, so a process that never calls out never reads the OS trust store. A refused certificate is explained with
//! the flag that fixes it.
//!
//! **Timeouts** are the caller's (each service's own, as AWS documents them): the whole exchange, connecting included, must finish in
//! [`Request::timeout`]. Callers run requests off their actor, so nothing waits on one but the delivery itself.
//!
//! **At most [`MAX_UNDER_WAY`] requests are under way at once**, in the whole process: SNS and EventBridge may each have ten thousand
//! deliveries waiting, and without a cap a fan-out opened as many sockets (and asked the resolver as often) at once. A request waits
//! for its turn before its timeout starts. **Names**: an IP address and `localhost` are connected to without asking the resolver
//! (`getaddrinfo` blocks, so it runs on the blocking pool); other names are resolved for each request. Connections are still not
//! kept: a delivery to the same place is a new connection, which a local endpoint answers in well under a millisecond.

mod answer;
pub mod jwks;
pub mod jwt;
mod os_roots;
mod tls;
mod url;

#[cfg(any(test, feature = "test-server"))]
pub mod test_server;

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::OnceLock;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::{OnceCell, Semaphore};

pub use tls::Trust;

/// The most requests under way at once, in the whole process.
pub const MAX_UNDER_WAY: usize = 64;

/// A request to send.
#[derive(Debug)]
pub struct Request<'a> {
    pub method: &'a str,
    /// `http://host[:port]/path?query` or `https://…`.
    pub url: &'a str,
    pub headers: &'a [(String, String)],
    pub body: &'a [u8],
    /// The most the whole exchange may take, connecting and the TLS handshake included.
    pub timeout: Duration,
    /// The most bytes of the answer read; past it the call fails (unless only the status is wanted).
    pub limit: usize,
    /// Read the status line and stop (a delivery that only judges the status).
    pub status_only: bool,
    /// Accept a certificate no trusted authority signed, if it is for the name called (API Gateway's
    /// `tlsConfig.insecureSkipVerification`).
    pub skip_verification: bool,
}

impl<'a> Request<'a> {
    /// A request with no headers or body, five seconds and 1 MiB.
    #[must_use]
    pub fn new(method: &'a str, url: &'a str) -> Self {
        Self {
            method,
            url,
            headers: &[],
            body: &[],
            timeout: Duration::from_secs(5),
            limit: 1024 * 1024,
            status_only: false,
            skip_verification: false,
        }
    }
}

/// What came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    /// The status line's reason phrase (`Not Found`), as the server wrote it; empty when only the status was read.
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Why no answer came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The URL is not `http://` or `https://`.
    Unsupported(String),
    /// The connection could not be made, or broke.
    Connect(String),
    /// The server's certificate was refused (the message names the flag that fixes it), or the handshake failed.
    Tls(String),
    /// No answer in time.
    Timeout(String),
    /// The answer is not HTTP, or is larger than the limit.
    Protocol(String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (Self::Unsupported(m)
        | Self::Connect(m)
        | Self::Tls(m)
        | Self::Timeout(m)
        | Self::Protocol(m)) = self;
        f.write_str(m)
    }
}

/// A client: its trust, and the TLS configuration built from it on first use.
#[derive(Debug)]
pub struct Client {
    trust: Trust,
    tls: OnceCell<Result<tls::Connectors, String>>,
    turns: Semaphore,
}

impl Client {
    #[must_use]
    pub fn new(trust: Trust) -> Self {
        Self {
            trust,
            tls: OnceCell::new(),
            turns: Semaphore::new(MAX_UNDER_WAY),
        }
    }

    /// How many requests may start now (for tests).
    #[cfg(test)]
    pub(crate) fn free_turns(&self) -> usize {
        self.turns.available_permits()
    }

    /// Makes the request.
    ///
    /// # Errors
    /// No answer came back: see [`Failure`].
    pub async fn fetch(&self, r: &Request<'_>) -> Result<Answer, Failure> {
        let Some(t) = url::split(r.url) else {
            return Err(Failure::Unsupported(format!(
                "{} is not an http:// or https:// URL",
                r.url
            )));
        };
        let at = format!("{}:{}", t.host, t.port);
        // WHY: the semaphore is never closed, so acquiring cannot fail.
        let Ok(_turn) = self.turns.acquire().await else {
            return Err(Failure::Connect("the client is closed".to_owned()));
        };
        let exchange = async {
            let wire = answer::wire(r, t.authority, &t.path);
            let mut tcp = connect(t.host, t.port)
                .await
                .map_err(|e| Failure::Connect(format!("could not connect to {at}: {e}")))?;
            if !t.tls {
                return answer::exchange(&mut tcp, r, &wire).await;
            }
            let connectors = self.connectors().await?;
            let connector = if r.skip_verification {
                &connectors.lenient
            } else {
                &connectors.strict
            };
            let name = tls::server_name(t.host).map_err(Failure::Tls)?;
            let mut stream = connector
                .connect(name, tcp)
                .await
                .map_err(|e| Failure::Tls(tls::explain(t.host, t.port, &e)))?;
            answer::exchange(&mut stream, r, &wire).await
        };
        tokio::time::timeout(r.timeout, exchange)
            .await
            .map_err(|_| Failure::Timeout(format!("no answer from {at} in {:?}", r.timeout)))?
    }

    async fn connectors(&self) -> Result<tls::Connectors, Failure> {
        let built = self
            .tls
            .get_or_init(|| async {
                let trust = self.trust.clone();
                // Reading the OS store touches the disk and, on macOS, the keychain: not on the runtime's thread.
                tokio::task::spawn_blocking(move || tls::connectors(&trust))
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()))
            })
            .await;
        built
            .clone()
            .map_err(|e| Failure::Tls(format!("outbound TLS is not available: {e}")))
    }
}

/// A connection to `host`: an address and `localhost` (its IPv4 address, then its IPv6 one) without the resolver, a name through it.
async fn connect(host: &str, port: u16) -> std::io::Result<TcpStream> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return TcpStream::connect(SocketAddr::new(ip, port)).await;
    }
    if host.eq_ignore_ascii_case("localhost") {
        let v4 = TcpStream::connect(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port)).await;
        return match v4 {
            Ok(s) => Ok(s),
            Err(e) => TcpStream::connect(SocketAddr::new(Ipv6Addr::LOCALHOST.into(), port))
                .await
                .map_err(|_| e),
        };
    }
    TcpStream::connect((host, port)).await
}

static CLIENT: OnceLock<Client> = OnceLock::new();

/// Sets the trust every service's requests use. The first call wins (doze-aws makes it once, at start, before any service runs);
/// without it, the OS trust store alone is used.
pub fn configure(trust: Trust) -> bool {
    CLIENT.set(Client::new(trust)).is_ok()
}

fn client() -> &'static Client {
    CLIENT.get_or_init(|| Client::new(default_trust()))
}

/// Unconfigured: the OS trust store.
#[cfg(not(feature = "test-server"))]
fn default_trust() -> Trust {
    Trust {
        os_store: true,
        ..Trust::default()
    }
}

/// Unconfigured in a test build: the test CA alone, whichever test makes the first request.
#[cfg(feature = "test-server")]
fn default_trust() -> Trust {
    test_server::Ca::get().trust()
}

/// Makes the request with the process's client.
///
/// # Errors
/// No answer came back: see [`Failure`].
pub async fn fetch(r: &Request<'_>) -> Result<Answer, Failure> {
    client().fetch(r).await
}

/// For the fuzz targets.
pub mod fuzz {
    /// An HTTP answer read from any bytes.
    #[must_use]
    pub fn http_answer(raw: &[u8]) -> bool {
        crate::answer::parse(raw).is_ok()
    }

    /// A URL taken apart.
    #[must_use]
    pub fn url(text: &str) -> bool {
        crate::url::split(text).is_some()
    }

    /// A JSON Web Token taken apart.
    #[must_use]
    pub fn jwt(text: &str) -> bool {
        crate::jwt::decode(text).is_some()
    }
}

#[cfg(test)]
mod tests;
