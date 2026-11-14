//! The in-process client (feature `tls`): `ureq` over `rustls` with `ring`, certificates checked against this machine's
//! own trust store (the keychain, /etc/ssl, the Windows store), so a proxy a company trusts is trusted here too. Not a
//! bundled root list: `webpki-roots` is CDLA-Permissive-2.0, outside the licences deny.toml allows.

use super::{Failure, TOO_BIG, UNTRUSTED, dns_failure, timed_out};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use ureq::tls::{Certificate, RootCerts, TlsConfig};

pub(super) fn get(url: &str, timeout: Duration, limit: u64) -> Result<Vec<u8>, Failure> {
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
        ureq::Error::Timeout(_) => timed_out(timeout),
        ureq::Error::ConnectionFailed => "the connection was refused".into(),
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => timed_out(timeout),
        // A failed name lookup reaches here as an uncategorised I/O error carrying getaddrinfo's words.
        ureq::Error::Io(io) if dns_failure(&io.to_string()) => "no DNS answer".into(),
        ureq::Error::Io(io) => format!("the connection failed ({io})"),
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => UNTRUSTED.into(),
        ureq::Error::BodyExceedsLimit(_) => TOO_BIG.into(),
        ureq::Error::ConnectProxyFailed(why) => format!("the proxy refused the connection ({why})"),
        // ureq's error is non-exhaustive: anything else is said as it is, with what failed.
        other => format!("the request failed ({other})"),
    }
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
