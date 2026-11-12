//! A request in a REST protocol (S3): the same steps as a JSON one (authenticate, route, decode and check against the model, call,
//! answer), with the operation found from the method, path and query, the input read from path, query, headers and an XML body,
//! and the payload of an upload or a download streamed instead of held.

use aws_auth::Pending;
use aws_auth::claim::Claim;
use aws_schema::arena::{Arena, NIL};
use aws_schema::json::decode;
use aws_schema::tables::{K_STREAM, Tables};
use aws_wire::rest::request::{Parts as Input, ReadError, to_json};
use aws_wire::rest::response::{Rendered, error, host_id, render};
use aws_wire::rest::route::Matched;
use aws_wire::rest::{RestReply, Upload};
use aws_wire::service::{Call, Caller};
use aws_wire::{ApiError, Family, RequestId, from_auth};
use http::request::Parts;
use http::{HeaderValue, Method, Response, header};
use hyper::body::Incoming;

use crate::body::RespBody;
use crate::pipeline::{Entry, Shared, authenticate, read_body, session_token};
use crate::presign::{check_presigned, pairs};
use crate::upload::{Framing, upload};

/// Answers a request to the REST service `entry`.
pub(crate) async fn respond(
    shared: &Shared,
    entry: &Entry,
    id: &RequestId,
    parts: &Parts,
    body: Incoming,
    claim: Claim,
) -> Response<RespBody> {
    let head = parts.method == Method::HEAD;
    match handle(shared, entry, id, parts, body, claim).await {
        Ok(response) => response,
        Err(e) => finish(
            error(&e, id, head),
            id,
            None,
            e.status == 501 && e.code == "NotImplemented",
        ),
    }
}

async fn handle(
    shared: &Shared,
    entry: &Entry,
    id: &RequestId,
    parts: &Parts,
    body: Incoming,
    claim: Claim,
) -> Result<Response<RespBody>, ApiError> {
    let tables = entry.service.tables();
    let pending = authenticate(shared, parts, &claim.service)
        .await
        .map_err(|e| from_auth(&e, Family::S3))?;
    if pending.is_none() {
        check_presigned(&claim, parts)?;
    }
    let (path, host) = target(parts);
    let query = pairs(parts.uri.query().unwrap_or_default());
    let matched = entry
        .router(tables)
        .find(parts.method.as_str(), &path, &query, |h| {
            parts.headers.contains_key(h)
        })
        .ok_or_else(unrouted)?;
    let op = matched.op;
    if !entry.service.serves(op) {
        let name = tables.op(op).map_or("", |o| o.name);
        return Err(ApiError::new(
            501,
            "NotImplemented",
            format!("{name} is not built in doze-aws."),
        ));
    }
    let streaming = streams(tables, op);
    let (body_bytes, up) = take_body(shared, parts, body, pending, streaming).await?;
    let head = Head {
        id,
        parts,
        host: &host,
        account: shared.account_id(),
        matched: &matched,
        query: &query,
        body: &body_bytes,
    };
    let call = decode_call(entry, &head, claim)?;
    let rest = entry
        .service
        .rest()
        .ok_or_else(|| ApiError::internal("the service has no REST side"))?;
    let reply = rest.call_rest(call, up).await?;
    Ok(answer(entry, id, op, parts.method == Method::HEAD, reply))
}

fn unrouted() -> ApiError {
    ApiError::new(
        405,
        "MethodNotAllowed",
        "The specified method is not allowed against this resource.",
    )
}

/// The path to route (the bucket put back in front of a virtual-host path) and the host without its bucket label.
fn target(parts: &Parts) -> (String, String) {
    let host = parts
        .headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost");
    let path = parts.uri.path();
    match bucket_of_host(host) {
        Some((bucket, rest)) => {
            let trimmed = path.trim_start_matches('/');
            (format!("/{bucket}/{trimmed}"), rest)
        }
        None => (path.to_owned(), host.to_owned()),
    }
}

/// `bucket.s3.amazonaws.com` or `bucket.s3.us-west-2.amazonaws.com:4566`: the bucket, and the host that remains. A host with no
/// `s3` label after at least one other label (and any IP address) is a path-style host.
fn bucket_of_host(host: &str) -> Option<(String, String)> {
    let name = host.rsplit_once(':').map_or(host, |(n, p)| {
        if p.bytes().all(|b| b.is_ascii_digit()) {
            n
        } else {
            host
        }
    });
    if name.parse::<std::net::IpAddr>().is_ok() || name.starts_with('[') {
        return None;
    }
    let labels: Vec<&str> = name.split('.').collect();
    let at = labels
        .iter()
        .position(|l| *l == "s3" || l.starts_with("s3-"))
        .filter(|at| *at > 0)?;
    let bucket = labels.get(..at)?.join(".");
    let rest = host.get(bucket.len() + 1..)?.to_owned();
    Some((bucket, rest))
}

/// Whether the operation's input has a streaming blob.
fn streams(tables: &Tables, op: u32) -> bool {
    let Some(shape) = tables
        .op(op)
        .and_then(|o| o.input)
        .and_then(|s| tables.shape(s))
    else {
        return false;
    };
    (shape.first..shape.first + shape.count)
        .filter_map(|i| tables.member(i))
        .any(|m| m.kind == K_STREAM)
}

/// The body: read whole for an operation without a stream, or opened as an upload (and the signature finished) for one with.
async fn take_body(
    shared: &Shared,
    parts: &Parts,
    body: Incoming,
    pending: Option<Pending>,
    streaming: bool,
) -> Result<(bytes::Bytes, Option<Box<dyn Upload>>), ApiError> {
    if !streaming {
        let bytes = read_body(body, shared.max_body_bytes()).await?;
        if let Some(pending) = pending {
            let hash = pending
                .needs_body_hash()
                .then(|| aws_auth::key::sha256_hex(&bytes));
            pending
                .finish(parts, hash.as_deref())
                .map_err(|e| from_auth(&e, Family::S3))?;
        }
        return Ok((bytes, None));
    }
    let (framing, length) = framing_of(parts, pending)?;
    let claimed = parts
        .headers
        .get("x-amz-content-sha256")
        .and_then(|v| v.to_str().ok())
        .filter(|h| h.len() == 64)
        .map(str::to_owned);
    Ok((
        bytes::Bytes::new(),
        Some(upload(body, framing, length, claimed)),
    ))
}

/// How the payload is framed, and its declared length. The signature is finished here, before a byte of the payload is read.
fn framing_of(parts: &Parts, pending: Option<Pending>) -> Result<(Framing, Option<u64>), ApiError> {
    let header = |name: &str| parts.headers.get(name).and_then(|v| v.to_str().ok());
    let declared = header("x-amz-content-sha256").unwrap_or_default();
    let chunked = declared.starts_with("STREAMING-")
        || header("content-encoding").is_some_and(|e| e.contains("aws-chunked"));
    let length: Option<u64> = if chunked {
        header("x-amz-decoded-content-length").and_then(|v| v.parse().ok())
    } else {
        header("content-length").and_then(|v| v.parse().ok())
    };
    let signed_trailer = declared.starts_with("STREAMING-AWS4-HMAC-SHA256-PAYLOAD-TRAILER");
    let mut auth = None;
    if let Some(pending) = pending {
        if pending.needs_body_hash() {
            return Err(ApiError::new(
                400,
                "InvalidArgument",
                "x-amz-content-sha256 must be UNSIGNED-PAYLOAD, STREAMING-UNSIGNED-PAYLOAD-TRAILER, STREAMING-AWS4-HMAC-SHA256-PAYLOAD, STREAMING-AWS4-HMAC-SHA256-PAYLOAD-TRAILER or a valid sha256 value.",
            ));
        }
        let mut verified = pending
            .finish(parts, None)
            .map_err(|e| from_auth(&e, Family::S3))?;
        auth = verified.take_chunk_auth();
    }
    let framing = if chunked {
        Framing::Chunked(auth, signed_trailer)
    } else {
        Framing::Plain
    };
    Ok((framing, length))
}

/// What of the request the call is made from.
struct Head<'a> {
    id: &'a RequestId,
    parts: &'a Parts,
    host: &'a str,
    account: &'a str,
    matched: &'a Matched,
    query: &'a [(String, String)],
    body: &'a [u8],
}

fn decode_call(entry: &Entry, h: &Head<'_>, claim: Claim) -> Result<Call, ApiError> {
    let (matched, parts, id) = (h.matched, h.parts, h.id);
    let tables = entry.service.tables();
    let input = tables.op(matched.op).and_then(|o| o.input);
    let mut arena = Arena::new();
    let (text, root) = match input {
        Some(shape) => {
            let p = Input {
                labels: &matched.labels,
                query: h.query,
                headers: &parts.headers,
                body: h.body,
            };
            let json = to_json(tables, shape, &p).map_err(|ReadError::MalformedXml| {
                ApiError::new(
                    400,
                    "MalformedXML",
                    "The XML you provided was not well-formed or did not validate against our published schema",
                )
            })?;
            let text = json.to_string();
            let root = decode(tables, &entry.patterns, shape, &text, &mut arena)
                .map_err(|v| entry.service.invalid(&v))?;
            (text, root)
        }
        None => (String::new(), NIL),
    };
    Ok(Call {
        op: matched.op,
        id: id.clone(),
        caller: Caller {
            access_key_id: claim.access_key_id,
            account_id: h.account.to_owned(),
            region: claim.region,
            endpoint: format!("http://{}", h.host),
            service: None,
            session_token: session_token(parts),
        },
        body: text,
        arena,
        root,
    })
}

/// The HTTP answer for a service's reply.
fn answer(
    entry: &Entry,
    id: &RequestId,
    op: u32,
    head: bool,
    reply: RestReply,
) -> Response<RespBody> {
    let meta = entry.service.meta();
    let RestReply { json, status, body } = reply;
    let mut r = render(
        entry.service.tables(),
        meta.xml_namespace,
        op,
        &json,
        status,
    );
    if !r.body.is_empty()
        && !r
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
    {
        r.headers
            .push(("content-type".to_owned(), "application/xml".to_owned()));
    }
    if head {
        r.body.clear();
        return finish(r, id, None, false);
    }
    finish(r, id, body, false)
}

/// The response: status, headers, the identifying headers every S3 answer has, and the body.
fn finish(
    r: Rendered,
    id: &RequestId,
    stream: Option<aws_wire::rest::Download>,
    not_built: bool,
) -> Response<RespBody> {
    let mut builder = Response::builder().status(r.status);
    for (k, v) in &r.headers {
        if let (Ok(name), Ok(value)) = (
            header::HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            builder = builder.header(name, value);
        }
    }
    builder = builder
        .header("x-amz-request-id", id.as_str())
        .header("x-amz-id-2", host_id(id))
        .header(header::SERVER, "AmazonS3");
    if not_built {
        builder = builder.header("x-doze-not-built", "1");
    }
    let body = match stream {
        Some(s) => RespBody::Stream(s),
        None => RespBody::whole(r.body),
    };
    builder
        .body(body)
        .unwrap_or_else(|_| Response::new(RespBody::whole(Vec::new())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_virtual_host_names_the_bucket() {
        let cases = [
            ("b.s3.amazonaws.com", Some(("b", "s3.amazonaws.com"))),
            (
                "my.bucket.s3.us-west-2.amazonaws.com",
                Some(("my.bucket", "s3.us-west-2.amazonaws.com")),
            ),
            ("b.s3.localhost:4566", Some(("b", "s3.localhost:4566"))),
            (
                "b.s3-us-west-2.amazonaws.com",
                Some(("b", "s3-us-west-2.amazonaws.com")),
            ),
            ("s3.amazonaws.com", None),
            ("127.0.0.1:4566", None),
            ("localhost:4566", None),
            ("[::1]:4566", None),
        ];
        for (host, want) in cases {
            assert_eq!(
                bucket_of_host(host),
                want.map(|(b, r)| (b.to_owned(), r.to_owned())),
                "{host}"
            );
        }
    }

    #[test]
    fn a_query_is_decoded_in_order() {
        assert_eq!(
            pairs("prefix=a%2Fb+c&uploads&max-keys=5"),
            [("prefix", "a/b c"), ("uploads", ""), ("max-keys", "5")]
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
        );
    }
}
