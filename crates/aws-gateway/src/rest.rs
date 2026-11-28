//! A request in a REST protocol (S3): the same steps as a JSON one (authenticate, route, decode and check against the model, call,
//! answer), with the operation found from the method, path and query, the input read from path, query, headers and an XML body,
//! and the payload of an upload or a download streamed instead of held.

use aws_auth::Pending;
use aws_auth::claim::Claim;
use aws_schema::meta::Protocol;
use aws_schema::tables::{K_STREAM, Tables};
use aws_wire::rest::jsonout;
use aws_wire::rest::response::{Rendered, error, host_id, render};
use aws_wire::rest::{RestReply, Upload};
use aws_wire::{ApiError, Family, RequestId, from_auth};
use http::request::Parts;
use http::{HeaderValue, Method, Response, header};
use hyper::body::Incoming;

use crate::body::RespBody;
mod input;
use crate::fault::Step;
use crate::pipeline::{Entry, Shared, authenticate, read_body};
use crate::presign::{check_presigned, pairs};
use crate::s3host::target;
use crate::upload::{Framing, upload};
use input::{Head, decode_call};

/// Answers a request to the REST service `entry`.
pub(crate) async fn respond(
    shared: &Shared,
    entry: &Entry,
    id: &RequestId,
    parts: &Parts,
    body: Incoming,
    claim: Claim,
) -> Response<RespBody> {
    let region = claim.region.clone();
    let mut response = respond_inner(shared, entry, id, parts, body, claim).await;
    if !is_json(entry) && !crate::fault::is_dropped(&response) {
        crate::cors::decorate(entry, parts, &region, &mut response).await;
    }
    response
}

async fn respond_inner(
    shared: &Shared,
    entry: &Entry,
    id: &RequestId,
    parts: &Parts,
    body: Incoming,
    claim: Claim,
) -> Response<RespBody> {
    let head = parts.method == Method::HEAD;
    let json = is_json(entry);
    match handle(shared, entry, id, parts, body, claim).await {
        Ok(response) => response,
        // A Lab rule closed the connection: the answer is a mark, which the connection loop turns into no answer.
        Err(Halt::Dropped) => crate::fault::marked_dropped::<Vec<u8>>().map(RespBody::whole),
        Err(Halt::Error(e)) => {
            crate::observe::failed(parts, &e);
            let rendered = if json {
                jsonout::error(&e, entry.service.tables())
            } else {
                error(&e, id, head)
            };
            finish(
                rendered,
                id,
                json,
                None,
                e.status == 501 && e.code == "NotImplemented",
            )
        }
    }
}

/// Whether the service speaks restJson1 (Lambda) and not restXml (S3): they share this path and differ in how the input is read, the
/// answer written, and the headers every answer carries.
fn is_json(entry: &Entry) -> bool {
    entry.service.meta().speaks(Protocol::RestJson1)
}

/// How an authentication failure is worded for the protocol.
fn family(json: bool) -> Family {
    if json { Family::Coral } else { Family::S3 }
}

/// Why a request stops short of its answer.
enum Halt {
    /// It is answered with this error.
    Error(ApiError),
    /// A Lab rule closes the connection with no answer.
    Dropped,
}

impl From<ApiError> for Halt {
    fn from(e: ApiError) -> Self {
        Self::Error(e)
    }
}

async fn handle(
    shared: &Shared,
    entry: &Entry,
    id: &RequestId,
    parts: &Parts,
    body: Incoming,
    claim: Claim,
) -> Result<Response<RespBody>, Halt> {
    let tables = entry.service.tables();
    let json = is_json(entry);
    let pending = if crate::presign::is_sigv2(&claim) {
        crate::presign::verify_sigv2(shared, parts, &claim)
            .await
            .map_err(|e| from_auth(&e, family(json)))?;
        None
    } else if crate::presign::is_anonymous(&claim) {
        None
    } else {
        authenticate(shared, &claim)
            .await
            .map_err(|e| from_auth(&e, family(json)))?
    };
    if pending.is_none() {
        check_presigned(&claim, parts)?;
    }
    let (path, host) = address(entry, parts, json);
    let query = pairs(parts.uri.query().unwrap_or_default());
    let matched = entry
        .router(tables)
        .find(parts.method.as_str(), &path, &query, |h| {
            parts.headers.contains_key(h)
        })
        .ok_or_else(|| unrouted(json))?;
    let op = matched.op;
    crate::observe::operation(parts, entry, op);
    if !json {
        crate::observe::resource(parts, path.trim_start_matches('/'));
    }
    if !entry.service.serves(op) {
        let name = tables.op(op).map_or("", |o| o.name);
        return Err(ApiError::new(
            501,
            "NotImplemented",
            format!("{name} is not built in doze-aws."),
        )
        .into());
    }
    let streaming = streams(tables, op);
    let (body_bytes, up) = take_body(shared, parts, body, pending, streaming, family(json)).await?;
    let head = Head {
        id,
        parts,
        host: &host,
        account: shared.account_id(),
        matched: &matched,
        query: &query,
        body: &body_bytes,
        json,
    };
    let call = decode_call(entry, &head, claim)?;
    let rest = entry
        .service
        .rest()
        .ok_or_else(|| ApiError::internal("the service has no REST side"))?;
    match crate::fault::apply(shared, parts).await {
        Step::Go => {}
        Step::Fail(e) => return Err(Halt::Error(e)),
        Step::Drop => return Err(Halt::Dropped),
    }
    let reply = rest.call_rest(call, up).await?;
    Ok(answer(parts, entry, id, op, reply))
}

/// The path a request is routed by and the host it was sent to. A service called below a base path (the API Gateway management API
/// at its stage) routes what follows it; the base goes on with the host, into the caller's endpoint.
fn address(entry: &Entry, parts: &Parts, json: bool) -> (String, String) {
    if !json {
        return target(parts);
    }
    let (host, full) = (host_of(parts), parts.uri.path());
    let cut = entry
        .service
        .rest()
        .map_or(0, |r| r.base_path(&host, full))
        .min(full.len());
    let (base, rest) = full.split_at(cut);
    (rest.to_owned(), format!("{host}{base}"))
}

fn unrouted(json: bool) -> ApiError {
    if json {
        return ApiError::new(404, "UnknownOperationException", "Unknown operation.");
    }
    ApiError::new(
        405,
        "MethodNotAllowed",
        "The specified method is not allowed against this resource.",
    )
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
    pending: Option<Pending<'_>>,
    streaming: bool,
    family: Family,
) -> Result<(bytes::Bytes, Option<Box<dyn Upload>>), ApiError> {
    if !streaming {
        let bytes = read_body(body, shared.max_body_bytes()).await?;
        if let Some(pending) = pending {
            let hash = pending
                .needs_body_hash()
                .then(|| aws_auth::key::sha256_hex(&bytes));
            pending
                .finish(parts, hash.as_deref())
                .map_err(|e| from_auth(&e, family))?;
        }
        return Ok((bytes, None));
    }
    let (framing, length) = framing_of(parts, pending, family)?;
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
fn framing_of(
    parts: &Parts,
    pending: Option<Pending<'_>>,
    family: Family,
) -> Result<(Framing, Option<u64>), ApiError> {
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
            .map_err(|e| from_auth(&e, family))?;
        auth = verified.take_chunk_auth();
    }
    let framing = if chunked {
        Framing::Chunked(auth, signed_trailer)
    } else {
        Framing::Plain
    };
    Ok((framing, length))
}

/// The `Host` header, or `localhost`.
fn host_of(parts: &Parts) -> String {
    parts
        .headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost")
        .to_owned()
}

/// The HTTP answer for a service's reply.
fn answer(
    parts: &Parts,
    entry: &Entry,
    id: &RequestId,
    op: u32,
    mut reply: RestReply,
) -> Response<RespBody> {
    crate::observe::output_json(parts, entry, &reply.json);
    let head = parts.method == Method::HEAD;
    let meta = entry.service.meta();
    let wire_json = is_json(entry);
    // A payload held whole (Lambda's `Invoke`) is the body as it is, with a known length: no base64 in the JSON and back.
    let whole = if wire_json { reply.take_whole() } else { None };
    let RestReply { json, status, body } = reply;
    let mut r = if wire_json {
        jsonout::render_with(entry.service.tables(), op, &json, status, whole)
    } else {
        render(
            entry.service.tables(),
            meta.xml_namespace,
            op,
            &json,
            status,
        )
    };
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
        return finish(r, id, wire_json, None, false);
    }
    finish(r, id, wire_json, body, false)
}

/// The response: status, headers, the identifying headers every S3 answer has, and the body.
fn finish(
    r: Rendered,
    id: &RequestId,
    json: bool,
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
    builder = if json {
        builder.header("x-amzn-RequestId", id.as_str())
    } else {
        builder
            .header("x-amz-request-id", id.as_str())
            .header("x-amz-id-2", host_id(id))
            .header(header::SERVER, "AmazonS3")
    };
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
    fn a_query_is_decoded_in_order() {
        assert_eq!(
            pairs("prefix=a%2Fb+c&uploads&max-keys=5"),
            [("prefix", "a/b c"), ("uploads", ""), ("max-keys", "5")]
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
        );
    }
}
