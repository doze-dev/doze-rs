//! One blocking GET with a deadline, and its failure in words a person can act on ("no DNS answer", "timed out after
//! 3s"), never the library's error alone.

use std::time::Duration;

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
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .user_agent(concat!("doze-cli/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut resp = agent
        .get(url)
        .call()
        .map_err(|e| Failure::Unreachable(words(&e, timeout)))?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(Failure::Status(status));
    }
    resp.body_mut()
        .with_config()
        .limit(LIMIT)
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
