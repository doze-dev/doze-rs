//! doze's one HTTP client: one blocking GET with a deadline, and its failure in words a person can act on ("no DNS
//! answer", "no answer within 3s"), never the library's error alone. The registry and the release file go through it,
//! and so does the desktop app's update check, so a binary carries one HTTP stack, not two.
//!
//! Two ways to make the request, chosen when the binary is built (docs/port/weight.md, "One HTTP stack"):
//! - feature `tls` (the `doze` binary): in the process, `ureq` over `rustls` with this machine's trust store, so `doze`
//!   needs nothing installed beside it, on a server or in a container;
//! - without it (the desktop app): the system's `curl`, which macOS and Windows ship with the OS. The app saves the
//!   0.84 MB an HTTPS stack costs in its binary, where every byte is counted against 16 MB.

// Always built, so its tests run in every build; used only without `tls`.
#[cfg_attr(feature = "tls", allow(dead_code))]
mod curl;
#[cfg(not(feature = "tls"))]
use curl as client;
#[cfg(feature = "tls")]
mod tls;
#[cfg(feature = "tls")]
use tls as client;

use std::time::Duration;

/// The most a GET may bring back: indexes, docs and release files are a few KiB.
pub const LIMIT: u64 = 4 << 20;

/// How a GET ended, when it did not end with the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// Nothing answered: why, in words.
    Unreachable(String),
    /// The host answered with this status.
    Status(u16),
}

/// GETs `url`, giving up after `timeout` in all; the body when the answer is 200.
pub fn get(url: &str, timeout: Duration) -> Result<Vec<u8>, Failure> {
    client::get(url, timeout)
}

/// The words for a request that timed out.
fn timed_out(timeout: Duration) -> String {
    format!("no answer within {}s", timeout.as_secs_f32())
}

/// Whether an error's text is a resolver's "no such name" (macOS, glibc, Windows and curl spell it differently).
fn dns_failure(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    [
        "lookup address",
        "name or service not known",
        "nodename nor servname",
        "no such host",
        "temporary failure in name resolution",
        "could not resolve host",
    ]
    .iter()
    .any(|m| t.contains(m))
}

/// What both clients say when a certificate is refused.
const UNTRUSTED: &str = "its certificate was not trusted (a proxy in between?)";
/// What both clients say when the body passes [`LIMIT`].
const TOO_BIG: &str = "it sent far more than a registry file";

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
mod tests {
    use super::tests_support::serve;
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn a_200_brings_the_body() {
        let url = serve("200 OK", b"schema: 1\n");
        assert_eq!(get(&url, Duration::from_secs(5)).unwrap(), b"schema: 1\n");
    }

    #[test]
    fn another_status_is_said_as_the_status() {
        let url = serve("404 Not Found", b"no");
        assert_eq!(get(&url, Duration::from_secs(5)), Err(Failure::Status(404)));
    }

    #[test]
    fn a_closed_port_is_a_refused_connection() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let got = get(
            &format!("http://127.0.0.1:{port}/x"),
            Duration::from_secs(5),
        );
        let Err(Failure::Unreachable(why)) = got else {
            panic!("{got:?}")
        };
        assert!(
            why.contains("refused") || why.contains("connection failed"),
            "{why}"
        );
    }

    #[test]
    fn a_silent_host_is_a_timeout_in_words() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/x", l.local_addr().unwrap());
        let got = get(&url, Duration::from_millis(400));
        drop(l);
        assert_eq!(
            got,
            Err(Failure::Unreachable(timed_out(Duration::from_millis(400))))
        );
    }

    #[test]
    fn resolver_words_are_recognised() {
        assert!(dns_failure(
            "curl: (6) Could not resolve host: registry.doze-dev.com"
        ));
        assert!(dns_failure(
            "failed to lookup address information: nodename nor servname provided"
        ));
        assert!(!dns_failure("connection reset by peer"));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests_support {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A one-shot local server answering `status` with `body`; its URL.
    pub(super) fn serve(status: &'static str, body: &'static [u8]) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/index.yaml", l.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut req = [0u8; 2048];
            let _ = s.read(&mut req);
            let head = format!(
                "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(body);
        });
        url
    }
}
