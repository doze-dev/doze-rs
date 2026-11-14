//! The request written and the answer read: HTTP/1.1, one exchange per connection (`Connection: close`), the answer framed by
//! `Content-Length`, chunked encoding, or the end of the connection.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{Answer, Failure, Request};

/// The request's bytes: the head with the caller's headers, then ours, then the body.
pub(crate) fn wire(r: &Request<'_>, authority: &str, path: &str) -> Vec<u8> {
    let mut head = format!("{} {path} HTTP/1.1\r\nHost: {authority}\r\n", r.method);
    for (k, v) in r.headers {
        // A header value may not carry a line break; one that does is cut at it.
        let v = v.split(['\r', '\n']).next().unwrap_or_default();
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        r.body.len()
    ));
    // One write for the head and the body: a server that reads the request in one go gets all of it.
    let mut out = head.into_bytes();
    out.extend_from_slice(r.body);
    out
}

/// Sends the request on `stream` and reads the answer.
pub(crate) async fn exchange<S>(stream: &mut S, r: &Request<'_>, bytes: &[u8]) -> Result<Answer, Failure>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    stream
        .write_all(bytes)
        .await
        .map_err(|e| Failure::Connect(format!("could not send the request: {e}")))?;
    stream.flush().await.ok();
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = match stream.read(&mut buf).await {
            Ok(n) => n,
            // A TLS server that closes without saying so (no close_notify) has still answered.
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof && !raw.is_empty() => 0,
            Err(e) => {
                return Err(Failure::Connect(format!("could not read the answer: {e}")));
            }
        };
        raw.extend_from_slice(buf.get(..n).unwrap_or_default());
        if n == 0 || complete(&raw, r.status_only) {
            break;
        }
        if raw.len() > r.limit {
            if r.status_only {
                break;
            }
            return Err(Failure::Protocol(format!(
                "the answer is larger than {} bytes",
                r.limit
            )));
        }
    }
    if r.status_only {
        return status_of(&raw).map(|status| Answer {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        });
    }
    parse(&raw)
}

/// Whether `raw` holds the whole answer (so a server that keeps the connection open is not waited for).
fn complete(raw: &[u8], status_only: bool) -> bool {
    if status_only {
        return raw.contains(&b'\n');
    }
    let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return false;
    };
    let head = String::from_utf8_lossy(raw.get(..end).unwrap_or_default()).to_ascii_lowercase();
    let body = raw.get(end + 4..).unwrap_or_default();
    if head.contains("transfer-encoding: chunked") {
        return body.ends_with(b"0\r\n\r\n");
    }
    head.lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .is_some_and(|len| body.len() >= len)
}

fn status_of(raw: &[u8]) -> Result<u16, Failure> {
    let line = String::from_utf8_lossy(raw);
    line.lines()
        .next()
        .filter(|l| l.starts_with("HTTP/"))
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| Failure::Protocol("the answer was not HTTP".to_owned()))
}

// fuzz:parser fuzz_http_answer
/// An HTTP/1.1 answer read from its bytes.
pub(crate) fn parse(raw: &[u8]) -> Result<Answer, Failure> {
    let bad = |why: &str| Failure::Protocol(why.to_owned());
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| bad("the answer has no end of headers"))?;
    let head = String::from_utf8_lossy(raw.get(..split).unwrap_or_default());
    let body = raw.get(split + 4..).unwrap_or_default();
    let status = status_of(head.as_bytes())?;
    let headers: Vec<(String, String)> = head
        .split("\r\n")
        .skip(1)
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let chunked = headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked")
    });
    let body = if chunked {
        dechunk(body).map_err(|w| bad(w))?
    } else {
        body.to_vec()
    };
    let headers = headers
        .into_iter()
        .filter(|(k, _)| {
            !k.eq_ignore_ascii_case("transfer-encoding") && !k.eq_ignore_ascii_case("connection")
        })
        .collect();
    Ok(Answer {
        status,
        headers,
        body,
    })
}

fn dechunk(mut b: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut out = Vec::new();
    loop {
        let line_end = b
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("a chunk is not framed")?;
        let size_text = String::from_utf8_lossy(b.get(..line_end).unwrap_or_default());
        let size =
            usize::from_str_radix(size_text.split(';').next().unwrap_or_default().trim(), 16)
                .map_err(|_| "a chunk size is not hex")?;
        b = b.get(line_end + 2..).unwrap_or_default();
        if size == 0 {
            return Ok(out);
        }
        out.extend_from_slice(b.get(..size).ok_or("a chunk is cut short")?);
        b = b.get(size + 2..).unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read_plain_and_chunked() {
        let a = parse(b"HTTP/1.1 201 Created\r\nX-A: 1\r\nContent-Length: 2\r\n\r\nhi").unwrap();
        assert_eq!((a.status, &a.body[..]), (201, &b"hi"[..]));
        let b = parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nhe\r\n3\r\nllo\r\n0\r\n\r\n").unwrap();
        assert_eq!(&b.body[..], b"hello");
        assert!(b.headers.is_empty());
        assert!(parse(b"garbage").is_err());
        assert!(parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n").is_err());
    }

    #[test]
    fn an_answer_is_complete_by_its_framing() {
        assert!(complete(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi", false));
        assert!(!complete(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nhi", false));
        assert!(complete(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n", false));
        assert!(!complete(b"HTTP/1.1 200 OK\r\n\r\nstreaming", false));
        assert!(complete(b"HTTP/1.1 204 No\r\n", true));
    }

    #[test]
    fn the_host_header_keeps_the_port() {
        let r = Request {
            body: b"x",
            ..Request::new("POST", "http://h:81/p")
        };
        let w = String::from_utf8(wire(&r, "h:81", "/p")).unwrap();
        assert!(w.starts_with("POST /p HTTP/1.1\r\nHost: h:81\r\n"), "{w}");
        assert!(w.ends_with("Content-Length: 1\r\nConnection: close\r\n\r\nx"), "{w}");
    }
}
