//! The one HTTP request a service makes to the outside (an EventBridge API destination, an SNS `http` subscription): HTTP/1.1 over a
//! plain TCP connection, one request, `Connection: close`, the answer's status line read and the rest ignored. There is no TLS client in
//! doze-aws (a size decision the owner has not made), so an `https://` endpoint is not called; the caller says so.
//!
//! The request waits at most five seconds; callers run it off their actor, so nothing waits on it but the delivery itself.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// How long a request may take, connecting and answering.
const TIMEOUT: Duration = Duration::from_secs(5);
/// How much of the answer is read: the status line is all that is used.
const READ_LIMIT: usize = 8 * 1024;

/// A request to send.
#[derive(Debug)]
pub struct Request<'a> {
    pub method: &'a str,
    /// `http://host:port/path?query`.
    pub url: &'a str,
    pub headers: &'a [(String, String)],
    pub body: &'a str,
}

/// Why nothing was sent, or what came back.
#[derive(Debug, PartialEq, Eq)]
pub enum Sent {
    /// The status the server answered.
    Status(u16),
    /// The request could not be made (connection refused, timeout, an answer that is not HTTP).
    Failed(String),
    /// The scheme is one this client does not speak.
    Unsupported(String),
}

/// `host`, `port` and `path?query` of an `http://` URL.
fn split(url: &str) -> Option<(&str, u16, String)> {
    let rest = url.strip_prefix("http://")?;
    let (authority, path) = rest
        .find(['/', '?'])
        .map_or((rest, "/"), |i| rest.split_at(i));
    let path = if path.starts_with('?') {
        format!("/{path}")
    } else {
        path.to_owned()
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().ok()?),
        None => (authority, 80),
    };
    (!host.is_empty()).then_some((host, port, path))
}

/// The request as bytes: the headers the caller gave, then ours.
fn head(r: &Request<'_>, host: &str, path: &str) -> String {
    let mut out = format!("{} {path} HTTP/1.1\r\nHost: {host}\r\n", r.method);
    for (k, v) in r.headers {
        // A header value may not carry a line break; one that does is cut at it.
        let v = v.split(['\r', '\n']).next().unwrap_or_default();
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        r.body.len()
    ));
    out
}

/// Makes the request.
pub async fn send(r: &Request<'_>) -> Sent {
    let Some((host, port, path)) = split(r.url) else {
        return Sent::Unsupported(format!(
            "{} is not an http:// URL; this client does not speak TLS",
            r.url
        ));
    };
    match tokio::time::timeout(TIMEOUT, exchange(r, host, port, &path)).await {
        Ok(Ok(status)) => Sent::Status(status),
        Ok(Err(why)) => Sent::Failed(why),
        Err(_) => Sent::Failed(format!("no answer from {host}:{port} in {TIMEOUT:?}")),
    }
}

async fn exchange(r: &Request<'_>, host: &str, port: u16, path: &str) -> Result<u16, String> {
    let mut stream = TcpStream::connect((host, port))
        .await
        .map_err(|e| format!("could not connect to {host}:{port}: {e}"))?;
    let wire = head(r, host, path) + r.body;
    stream
        .write_all(wire.as_bytes())
        .await
        .map_err(|e| format!("could not send: {e}"))?;
    let mut answer = Vec::new();
    let mut buf = [0u8; 1024];
    while answer.len() < READ_LIMIT && !answer.contains(&b'\n') {
        let n = stream
            .read(&mut buf)
            .await
            .map_err(|e| format!("could not read the answer: {e}"))?;
        if n == 0 {
            break;
        }
        answer.extend_from_slice(buf.get(..n).unwrap_or_default());
    }
    let line = String::from_utf8_lossy(&answer);
    let status = line
        .lines()
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse().ok());
    status.ok_or_else(|| "the answer was not HTTP".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_taken_apart() {
        assert_eq!(
            split("http://localhost:9000/a/b?c=1"),
            Some(("localhost", 9000, "/a/b?c=1".to_owned()))
        );
        assert_eq!(
            split("http://example.test"),
            Some(("example.test", 80, "/".to_owned()))
        );
        assert_eq!(split("http://h?x=1"), Some(("h", 80, "/?x=1".to_owned())));
        assert_eq!(split("https://example.test/"), None);
        assert_eq!(split("http://:80/"), None);
        assert_eq!(split("http://h:notaport/"), None);
    }
}
