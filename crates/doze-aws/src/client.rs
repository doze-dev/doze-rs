//! A small blocking HTTP/1.1 client for the commands that talk to a running doze-aws (`apply`, `export`, `doctor`): one request
//! per connection, SigV4-signed with aws-auth's own primitives. No HTTP client crate: the binary already speaks HTTP as a server,
//! and these calls are a handful of form posts to loopback.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use aws_auth::canonical::{Flavor, canonical_request, string_to_sign};
use aws_auth::key::{SigningKey, sha256_hex};

/// Where to call and as whom.
#[derive(Clone, Debug)]
pub(crate) struct Client {
    addr: SocketAddr,
    host: String,
    region: String,
    access_key: String,
    secret_key: String,
}

/// An answer.
#[derive(Debug)]
pub(crate) struct Reply {
    pub(crate) status: u16,
    pub(crate) body: Vec<u8>,
}

impl Client {
    /// A client of `endpoint` (`http://host:port`), signing for `region` with the keys.
    pub(crate) fn new(
        endpoint: &str,
        region: &str,
        access_key: &str,
        secret_key: &str,
    ) -> Result<Self, String> {
        let host = endpoint
            .strip_prefix("http://")
            .ok_or_else(|| format!("{endpoint} is not an http:// address"))?
            .trim_end_matches('/')
            .to_owned();
        let addr = host
            .to_socket_addrs()
            .ok()
            .and_then(|mut a| a.next())
            .ok_or_else(|| format!("{host} is not an address"))?;
        Ok(Self {
            addr,
            host,
            region: region.to_owned(),
            access_key: access_key.to_owned(),
            secret_key: secret_key.to_owned(),
        })
    }

    /// An unsigned `GET`.
    pub(crate) fn get(&self, path: &str, timeout: Duration) -> Result<Reply, String> {
        let head = format!(
            "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            self.host
        );
        self.exchange(head.as_bytes(), timeout)
    }

    /// A signed form post to an awsQuery service (`service` is its signing name).
    pub(crate) fn query(&self, service: &str, form: &[(String, String)]) -> Result<Reply, String> {
        let body = form
            .iter()
            .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let content_type = "application/x-www-form-urlencoded; charset=utf-8";
        let amz_date = aws_core::time::format_amz_date(now_secs());
        let auth = self.sign(service, content_type, &amz_date, body.as_bytes())?;
        let head = format!(
            "POST / HTTP/1.1\r\nHost: {}\r\nContent-Type: {content_type}\r\nX-Amz-Date: {amz_date}\r\nAuthorization: {auth}\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n",
            self.host,
            body.len()
        );
        let mut request = head.into_bytes();
        request.extend_from_slice(body.as_bytes());
        self.exchange(&request, Duration::from_secs(60))
    }

    fn sign(
        &self,
        service: &str,
        content_type: &str,
        amz_date: &str,
        body: &[u8],
    ) -> Result<String, String> {
        let request = http::Request::post("/")
            .header("content-type", content_type)
            .header("host", &self.host)
            .header("x-amz-date", amz_date)
            .body(())
            .map_err(|e| e.to_string())?;
        let (parts, ()) = request.into_parts();
        let signed: Vec<String> = ["content-type", "host", "x-amz-date"]
            .map(str::to_owned)
            .into();
        let flavor = Flavor {
            s3: false,
            presigned: false,
        };
        let canonical = canonical_request(&parts, &signed, &sha256_hex(body), flavor)
            .map_err(|e| e.to_string())?;
        let date = amz_date.get(..8).unwrap_or_default();
        let scope = format!("{date}/{}/{service}/aws4_request", self.region);
        let to_sign = string_to_sign(amz_date, &scope, &canonical);
        let signature = SigningKey::derive(&self.secret_key, date, &self.region, service)
            .and_then(|k| k.sign_hex(to_sign.as_bytes()))
            .ok_or("the request could not be signed")?;
        Ok(format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={}, Signature={signature}",
            self.access_key,
            signed.join(";")
        ))
    }

    fn exchange(&self, request: &[u8], timeout: Duration) -> Result<Reply, String> {
        let mut stream = TcpStream::connect_timeout(&self.addr, Duration::from_secs(2))
            .map_err(|e| format!("cannot reach doze-aws at {}: {e}", self.host))?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| e.to_string())?;
        stream.write_all(request).map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .map_err(|e| format!("doze-aws at {} did not answer: {e}", self.host))?;
        parse_response(&raw)
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

/// Form encoding as SigV4 and the Query protocol expect it (unreserved characters kept, the rest `%XX`).
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    aws_core::uri::encode_into(&mut out, s.as_bytes(), false);
    out
}

/// A response read to the end of the connection: its status and its body (de-chunked if it was chunked).
pub(crate) fn parse_response(raw: &[u8]) -> Result<Reply, String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("the answer has no end of headers")?;
    let head = String::from_utf8_lossy(raw.get(..split).unwrap_or_default());
    let body = raw.get(split + 4..).unwrap_or_default();
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or("the answer has no status")?;
    let chunked = head.lines().any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    let body = if chunked {
        dechunk(body)?
    } else {
        body.to_vec()
    };
    Ok(Reply { status, body })
}

fn dechunk(mut s: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let end = s
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("a chunk is cut short")?;
        let size_text = String::from_utf8_lossy(s.get(..end).unwrap_or_default());
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| "a chunk size is not hex")?;
        if size == 0 {
            return Ok(out);
        }
        let data = s
            .get(end + 2..end + 2 + size)
            .ok_or("a chunk is cut short")?;
        out.extend_from_slice(data);
        s = s.get(end + 4 + size..).unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_and_a_chunked_answer_are_read() {
        let r = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi").unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, &b"hi"[..]));
        let r = parse_response(b"HTTP/1.1 400 Bad\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nhe\r\n3\r\nllo\r\n0\r\n\r\n").unwrap();
        assert_eq!((r.status, r.body.as_slice()), (400, &b"hello"[..]));
    }

    #[test]
    fn form_values_are_percent_encoded() {
        assert_eq!(encode("a b/c=d&e~"), "a%20b%2Fc%3Dd%26e~");
    }
}
