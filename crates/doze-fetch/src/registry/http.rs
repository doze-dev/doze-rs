//! One blocking GET with a deadline, and its failure in words a person can act on ("no DNS answer", "timed out after
//! 3s"), never the library's error alone.
//!
//! Certificates are checked against this machine's own trust store (the keychain, /etc/ssl, the Windows store), so a
//! proxy a company trusts is trusted here too. Not a bundled root list: `webpki-roots` is CDLA-Permissive-2.0, outside
//! the licences deny.toml allows.

use std::sync::{Arc, OnceLock};
use std::time::Duration;
use ureq::tls::{Certificate, RootCerts, TlsConfig};

/// The most a registry file may be: indexes and docs are a few KiB.
const LIMIT: u64 = 4 << 20;

/// How a GET ended, when it did not end with the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// Nothing answered: why, in words.
    Unreachable(String),
    /// The host answered with this status.
    Status(u16),
}

/// GETs `url`, giving up after `timeout` in all.
pub(crate) fn get(url: &str, timeout: Duration) -> Result<Vec<u8>, Failure> {
    get_up_to(url, timeout, LIMIT)
}

/// The most an archive (an engine build, a module) may be.
pub(crate) const ARCHIVE_LIMIT: u64 = 1 << 30;

/// GETs `url` (or reads a `file://` path, as Go's fetcher did), up to `limit` bytes.
pub(crate) fn get_up_to(url: &str, timeout: Duration, limit: u64) -> Result<Vec<u8>, Failure> {
    if let Some(path) = url.strip_prefix("file://") {
        return std::fs::read(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Failure::Status(404),
            k => Failure::Unreachable(format!("cannot read it ({k})")),
        });
    }
    let mut resp = agent()
        .get(url)
        .config()
        .timeout_global(Some(timeout))
        .build()
        .call()
        .map_err(|e| Failure::Unreachable(words(&e, timeout)))?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(Failure::Status(status));
    }
    resp.body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|e| Failure::Unreachable(words(&e, timeout)))
}

fn words(e: &ureq::Error, timeout: Duration) -> String {
    match e {
        ureq::Error::HostNotFound => "no DNS answer".into(),
        ureq::Error::Timeout(_) => format!("no answer within {}s", timeout.as_secs_f32()),
        ureq::Error::ConnectionFailed => "the connection was refused".into(),
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => {
            format!("no answer within {}s", timeout.as_secs_f32())
        }
        // A failed name lookup reaches here as an uncategorised I/O error carrying getaddrinfo's words.
        ureq::Error::Io(io) if dns_failure(&io.to_string()) => "no DNS answer".into(),
        ureq::Error::Io(io) => format!("the connection failed ({io})"),
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => {
            "its certificate was not trusted (a proxy in between?)".into()
        }
        ureq::Error::BodyExceedsLimit(_) => "it sent far more than a registry file".into(),
        ureq::Error::ConnectProxyFailed(why) => format!("the proxy refused the connection ({why})"),
        // ureq's error is non-exhaustive: anything else is said as it is, with what failed.
        other => format!("the request failed ({other})"),
    }
}

/// Whether an I/O error's text is a resolver's "no such name" (macOS, glibc and Windows spell it differently).
fn dns_failure(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    [
        "lookup address",
        "name or service not known",
        "nodename nor servname",
        "no such host",
        "temporary failure in name resolution",
    ]
    .iter()
    .any(|m| t.contains(m))
}

/// One agent for the run, built on first use: loading the trust store is the slow part, and only a command that goes
/// out pays for it.
fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        let roots: Vec<Certificate<'static>> = rustls_native_certs::load_native_certs()
            .certs
            .iter()
            .map(|c| Certificate::from_der(c.as_ref()).to_owned())
            .collect();
        let tls = TlsConfig::builder()
            .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .root_certs(RootCerts::new_with_certs(&roots))
            .build();
        ureq::Agent::config_builder()
            .tls_config(tls)
            .http_status_as_error(false)
            .user_agent(concat!("doze/", env!("CARGO_PKG_VERSION")))
            .build()
            .into()
    })
}
