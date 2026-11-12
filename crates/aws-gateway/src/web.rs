//! Addresses a service owns that are not signed API calls (a Lambda function URL): asked before the gateway looks for a signature.

use aws_wire::service::{WebReply, WebRequest};
use aws_wire::{ApiError, RequestId};
use http::request::Parts;
use http::{HeaderValue, Response, header};
use hyper::body::Incoming;

use crate::body::RespBody;
use crate::pipeline::{Shared, endpoint_of, read_body};

/// Answers the request if a service owns its host and path; otherwise gives the body back untouched.
pub(crate) async fn serve(
    shared: &Shared,
    id: &RequestId,
    parts: &Parts,
    body: Incoming,
) -> Result<Response<RespBody>, Incoming> {
    let host = parts
        .headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();
    let path = parts.uri.path();
    // A browser's CORS preflight no service owns by address goes to the one that answers preflights (S3).
    let preflight = || {
        (parts.method == http::Method::OPTIONS)
            .then(|| shared.webs().find(|w| w.owns_preflight(host, path)))
            .flatten()
    };
    let Some(web) = shared
        .webs()
        .find(|w| w.owns(host, path))
        .or_else(preflight)
    else {
        return Err(body);
    };
    let bytes = match read_body(body, shared.max_body_bytes()).await {
        Ok(bytes) => bytes,
        Err(e) => return Ok(failure(id, &e)),
    };
    let request = WebRequest {
        method: parts.method.as_str().to_owned(),
        host: host.to_owned(),
        path: parts.uri.path().to_owned(),
        query: parts.uri.query().unwrap_or_default().to_owned(),
        headers: parts
            .headers
            .iter()
            .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
            .collect(),
        body: bytes,
        endpoint: endpoint_of(parts),
    };
    Ok(reply(id, web.serve(request).await))
}

fn reply(id: &RequestId, r: WebReply) -> Response<RespBody> {
    let mut builder = Response::builder().status(r.status);
    for (k, v) in &r.headers {
        if let (Ok(name), Ok(value)) = (
            header::HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            builder = builder.header(name, value);
        }
    }
    builder
        .header("x-amzn-RequestId", id.as_str())
        .body(RespBody::whole(r.body.to_vec()))
        .unwrap_or_else(|_| Response::new(RespBody::whole(Vec::new())))
}

fn failure(id: &RequestId, e: &ApiError) -> Response<RespBody> {
    reply(
        id,
        WebReply {
            status: e.status,
            headers: vec![("content-type".to_owned(), "application/json".to_owned())],
            body: format!("{{\"Message\":{:?}}}", e.message).into(),
        },
    )
}
