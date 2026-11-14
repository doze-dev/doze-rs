//! doze's one HTTP client: one blocking GET with a deadline, and its failure in words a person can act on ("no DNS
//! answer", "no answer within 3s"), never the library's error alone. The registry, the release file (the update
//! check), engine and module archives and the desktop app's update check all go through it, so a binary carries one
//! HTTP stack, not two. `doze-cli` re-exports it as `doze_cli::http`.
//!
//! Two ways to make the request, chosen when the binary is built (docs/port/weight.md, "One HTTP stack"):
//! - feature `tls` (the `doze` binary, the default): in the process, `ureq` over `rustls` with this machine's trust
//!   store, so `doze` needs nothing installed beside it, on a server or in a container;
//! - without it (the desktop app): the system's `curl`, which macOS and Windows ship with the OS, saving the
//!   0.84 MB an HTTPS stack costs in a binary where every byte counts against 16 MB.
//!
//! A `file://` address is read from disk in both builds (as Go's fetcher did), under the same size limit.

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

/// The most a GET may bring back by default: indexes, docs and release files are a few KiB.
pub const LIMIT: u64 = 4 << 20;

/// The most an archive (an engine build, a module) may be.
pub const ARCHIVE_LIMIT: u64 = 1 << 30;

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
    get_up_to(url, timeout, LIMIT)
}

/// GETs `url` (or reads a `file://` path), up to `limit` bytes.
pub fn get_up_to(url: &str, timeout: Duration, limit: u64) -> Result<Vec<u8>, Failure> {
    match url.strip_prefix("file://") {
        Some(path) => read_file(path, limit),
        None => client::get(url, timeout, limit),
    }
}

fn read_file(path: &str, limit: u64) -> Result<Vec<u8>, Failure> {
    let said = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::NotFound => Failure::Status(404),
        k => Failure::Unreachable(format!("cannot read it ({k})")),
    };
    // The same size limit as over the network: a manifest naming `file:///dev/zero` must not fill memory.
    let mut body = Vec::new();
    std::io::Read::read_to_end(
        &mut std::io::Read::take(std::fs::File::open(path).map_err(said)?, limit + 1),
        &mut body,
    )
    .map_err(said)?;
    if body.len() as u64 > limit {
        return Err(Failure::Unreachable(format!(
            "it is larger than {limit} bytes"
        )));
    }
    Ok(body)
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
/// What both clients say when the body passes its limit.
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
    fn a_body_over_the_limit_is_refused() {
        let url = serve("200 OK", b"0123456789");
        assert_eq!(
            get_up_to(&url, Duration::from_secs(5), 4),
            Err(Failure::Unreachable(TOO_BIG.into()))
        );
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

    /// F1 audit 3, minor 1: a `file://` body is held to the same limit as a download; a missing file is a 404.
    #[test]
    fn a_file_url_over_its_limit_is_refused() {
        let d = std::env::temp_dir().join(format!("doze-fetch-filelimit-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("big");
        std::fs::write(&f, vec![0u8; 4097]).unwrap();
        let url = format!("file://{}", f.display());
        let t = Duration::from_secs(1);
        assert_eq!(
            get_up_to(&url, t, 4096),
            Err(Failure::Unreachable("it is larger than 4096 bytes".into()))
        );
        assert_eq!(get_up_to(&url, t, 4097).map(|b| b.len()), Ok(4097));
        let missing = format!("file://{}", d.join("none").display());
        assert_eq!(get_up_to(&missing, t, 4096), Err(Failure::Status(404)));
        let _ = std::fs::remove_dir_all(&d);
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
