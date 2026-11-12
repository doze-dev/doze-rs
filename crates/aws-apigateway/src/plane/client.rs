//! HTTP integrations: one HTTP/1.1 request over a plain TCP connection (`Connection: close`), the answer read whole up to 10 MB with
//! `Content-Length`, chunked framing, or to the end of the connection. There is no TLS client in doze-aws (a size decision, as for SNS
//! and EventBridge), so an `https://` backend is refused with that reason rather than called.

use std::time::Duration;

use aws_wire::service::WebReply;
use bytes::Bytes;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::custom::{Backend, Mapped};
use super::mapping::rest_source;
use super::{Found, Incoming, Route, reply};
use crate::doc::text;

const MAX_ANSWER: usize = 10 * 1024 * 1024;

/// A request to send.
#[derive(Debug)]
pub(crate) struct Out {
    pub(crate) method: String,
    pub(crate) url: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Bytes,
    pub(crate) timeout: Duration,
}

/// What came back.
#[derive(Debug)]
pub(crate) struct In {
    pub(crate) status: u16,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Bytes,
}

/// Sends a request.
pub(crate) async fn call(out: &Out) -> Result<In, String> {
    let rest = out.url.strip_prefix("http://").ok_or_else(|| {
        format!(
            "{} is not an http:// URL; doze-aws has no TLS client",
            out.url
        )
    })?;
    let (authority, path) = rest
        .find(['/', '?'])
        .map_or((rest, "/"), |i| rest.split_at(i));
    let path = if path.starts_with('?') {
        format!("/{path}")
    } else {
        path.to_owned()
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (
            h,
            p.parse::<u16>()
                .map_err(|_| format!("bad port in {}", out.url))?,
        ),
        None => (authority, 80),
    };
    let exchange = async {
        let mut stream = TcpStream::connect((host, port))
            .await
            .map_err(|e| format!("could not connect to {authority}: {e}"))?;
        let mut head = format!("{} {path} HTTP/1.1\r\nHost: {authority}\r\n", out.method);
        for (k, v) in &out.headers {
            let v = v.split(['\r', '\n']).next().unwrap_or_default();
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str(&format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n",
            out.body.len()
        ));
        stream
            .write_all(head.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        stream
            .write_all(&out.body)
            .await
            .map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = stream.read(&mut buf).await.map_err(|e| e.to_string())?;
            if n == 0 || raw.len() > MAX_ANSWER {
                break;
            }
            raw.extend_from_slice(buf.get(..n).unwrap_or_default());
        }
        parse(&raw)
    };
    tokio::time::timeout(out.timeout, exchange)
        .await
        .map_err(|_| "Endpoint request timed out".to_owned())?
}

// fuzz:parser fuzz_http_answer
/// An HTTP/1.1 answer read from its bytes.
pub(crate) fn parse(raw: &[u8]) -> Result<In, String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("the answer has no end of headers")?;
    let head = String::from_utf8_lossy(raw.get(..split).unwrap_or_default());
    let body = raw.get(split + 4..).unwrap_or_default();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse().ok())
        .ok_or("the answer is not HTTP")?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let chunked = headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked")
    });
    let body = if chunked {
        dechunk(body)?
    } else {
        body.to_vec()
    };
    let headers = headers
        .into_iter()
        .filter(|(k, _)| {
            !k.eq_ignore_ascii_case("transfer-encoding") && !k.eq_ignore_ascii_case("connection")
        })
        .collect();
    Ok(In {
        status,
        headers,
        body: Bytes::from(body),
    })
}

fn dechunk(mut b: &[u8]) -> Result<Vec<u8>, String> {
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
        if out.len() > MAX_ANSWER {
            return Err("the answer is larger than 10 MB".into());
        }
        b = b.get(size + 2..).unwrap_or_default();
    }
}

/// The integration URI with `{name}` filled from the request's path variables and `integration.request.path` mappings, then the
/// mapped querystring added.
fn url_of(found: &Found, route: &Route, req: &Incoming, ctx: &Value, keep_query: bool) -> String {
    let vars = found
        .variables()
        .map_or(Value::Null, |m| Value::Object(m.clone()));
    let mut url = super::lambda::fill_vars(
        text(&route.integration, "uri").unwrap_or_default(),
        found.variables(),
    );
    let params = crate::doc::obj(&route.integration, "requestParameters");
    for (name, value) in &route.params {
        url = url
            .replace(&format!("{{{name}}}"), value)
            .replace(&format!("{{{name}+}}"), value);
    }
    let mut query: Vec<(String, String)> = if keep_query {
        req.query.clone()
    } else {
        Vec::new()
    };
    for (k, src) in params.into_iter().flatten() {
        let Some(v) = src
            .as_str()
            .and_then(|s| rest_source(s, req, &route.params, ctx, &vars))
        else {
            continue;
        };
        if let Some(p) = k.strip_prefix("integration.request.path.") {
            url = url
                .replace(&format!("{{{p}}}"), &v)
                .replace(&format!("{{{p}+}}"), &v);
        } else if let Some(q) = k.strip_prefix("integration.request.querystring.") {
            query.retain(|(n, _)| n != q);
            query.push((q.to_owned(), v));
        }
    }
    if !query.is_empty() {
        let q: Vec<String> = query
            .iter()
            .map(|(k, v)| {
                format!(
                    "{}={}",
                    aws_core::uri::encode(k.as_bytes(), false),
                    aws_core::uri::encode(v.as_bytes(), false)
                )
            })
            .collect();
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str(&q.join("&"));
    }
    url
}

fn mapped_headers(
    route: &Route,
    req: &Incoming,
    ctx: &Value,
    found: &Found,
) -> Vec<(String, String)> {
    let vars = found
        .variables()
        .map_or(Value::Null, |m| Value::Object(m.clone()));
    crate::doc::obj(&route.integration, "requestParameters")
        .into_iter()
        .flatten()
        .filter_map(|(k, src)| {
            let name = k.strip_prefix("integration.request.header.")?;
            Some((
                name.to_owned(),
                rest_source(src.as_str()?, req, &route.params, ctx, &vars)?,
            ))
        })
        .collect()
}

fn timeout(route: &Route) -> Duration {
    Duration::from_millis(
        route
            .integration
            .get("timeoutInMillis")
            .and_then(Value::as_u64)
            .unwrap_or(29_000),
    )
}

/// An HTTP_PROXY integration: the request passed on, the answer passed back.
pub(crate) async fn proxy(found: &Found, route: &Route, req: &Incoming) -> WebReply {
    let method = match text(&route.integration, "httpMethod") {
        Some(m) if m != "ANY" => m.to_owned(),
        _ => req.method.clone(),
    };
    let mut headers: Vec<(String, String)> = req
        .headers
        .iter()
        .filter(|(k, _)| {
            !matches!(
                k.as_str(),
                "host" | "connection" | "content-length" | "transfer-encoding"
            )
        })
        .cloned()
        .collect();
    headers.extend(mapped_headers(route, req, &Value::Null, found));
    let out = Out {
        method,
        url: url_of(found, route, req, &Value::Null, true),
        headers,
        body: req.body.clone(),
        timeout: timeout(route),
    };
    match call(&out).await {
        Ok(a) => reply::of(a.status, a.headers, a.body),
        Err(why) if why.contains("timed out") => reply::gateway(
            found,
            "INTEGRATION_TIMEOUT",
            504,
            "Endpoint request timed out",
            req,
            None,
        ),
        Err(why) => {
            tracing::warn!(
                "execute-api {}: HTTP integration failed: {why}",
                found.api_id()
            );
            reply::gateway(
                found,
                "INTEGRATION_FAILURE",
                500,
                "Internal server error",
                req,
                None,
            )
        }
    }
}

/// An HTTP (non-proxy) integration: the mapped body sent, the answer handed to the integration responses.
pub(crate) async fn send(
    found: &Found,
    route: &Route,
    req: &Incoming,
    mapped: &Mapped,
) -> Result<Backend, String> {
    let mut headers = mapped_headers(route, req, &mapped.context, found);
    if !headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
    {
        headers.push((
            "Content-Type".into(),
            req.header("content-type")
                .unwrap_or("application/json")
                .to_owned(),
        ));
    }
    let out = Out {
        method: text(&route.integration, "httpMethod")
            .unwrap_or("POST")
            .to_owned(),
        url: url_of(found, route, req, &mapped.context, false),
        headers,
        body: Bytes::from(mapped.body.clone()),
        timeout: timeout(route),
    };
    let a = call(&out).await?;
    Ok(Backend {
        selector: a.status.to_string(),
        status: Some(a.status),
        headers: a.headers,
        body: String::from_utf8_lossy(&a.body).into_owned(),
    })
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
        assert!(parse(b"garbage").is_err());
        assert!(parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n").is_err());
    }
}
