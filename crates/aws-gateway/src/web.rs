//! Addresses a service owns that are not signed API calls (a Lambda function URL): asked before the gateway looks for a signature.

use aws_wire::service::{BoxFuture, Socket, Upgrade, WebReply, WebRequest};
use aws_wire::{ApiError, RequestId};
use http::request::Parts;
use http::{HeaderValue, Response, header};
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::body::RespBody;
use crate::pipeline::{Shared, endpoint_of, read_body};

/// Answers the request if a service owns its host and path; otherwise gives the body back untouched. A request to upgrade the
/// connection (`Upgrade: websocket`) is offered to the service, which may take the connection over.
pub(crate) async fn serve(
    shared: &Shared,
    id: &RequestId,
    parts: &mut Parts,
    body: Incoming,
) -> Result<Response<RespBody>, Incoming> {
    let wants_upgrade = parts
        .headers
        .get(header::UPGRADE)
        .and_then(|u| u.to_str().ok())
        .is_some_and(|u| u.eq_ignore_ascii_case("websocket"));
    if wants_upgrade {
        return upgrade(shared, id, parts, body).await;
    }
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
    let request = request_of(parts, host, bytes);
    Ok(reply(id, web.serve(request).await))
}

fn request_of(parts: &Parts, host: &str, body: bytes::Bytes) -> WebRequest {
    WebRequest {
        method: parts.method.as_str().to_owned(),
        host: host.to_owned(),
        path: parts.uri.path().to_owned(),
        query: parts.uri.query().unwrap_or_default().to_owned(),
        headers: parts
            .headers
            .iter()
            .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
            .collect(),
        body,
        endpoint: endpoint_of(parts),
    }
}

/// An `Upgrade: websocket` request: the service that owns the address answers it, and if it takes the connection the session runs in
/// a tracked task once hyper hands the connection over (after the `101` is written).
async fn upgrade(
    shared: &Shared,
    id: &RequestId,
    parts: &mut Parts,
    body: Incoming,
) -> Result<Response<RespBody>, Incoming> {
    let host = parts
        .headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let Some(web) = shared.webs().find(|w| w.owns(&host, parts.uri.path())) else {
        return Err(body);
    };
    drop(body);
    let on_upgrade = parts.extensions.remove::<hyper::upgrade::OnUpgrade>();
    let request = request_of(parts, &host, bytes::Bytes::new());
    match web.upgrade(request).await {
        Upgrade::Refused(r) => Ok(reply(id, r)),
        Upgrade::Accepted { headers, session } => {
            if let Some(on) = on_upgrade {
                shared.tasks().spawn(async move {
                    if let Ok(upgraded) = on.await {
                        session(Box::new(Conn(TokioIo::new(upgraded)))).await;
                    }
                });
            }
            Ok(reply(
                id,
                WebReply {
                    status: 101,
                    headers,
                    body: bytes::Bytes::new(),
                },
            ))
        }
    }
}

/// An upgraded connection, as a service reads and writes it.
struct Conn(TokioIo<hyper::upgrade::Upgraded>);

impl Socket for Conn {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> BoxFuture<'a, std::io::Result<usize>> {
        Box::pin(async move { self.0.read(buf).await })
    }

    fn write_all<'a>(&'a mut self, buf: &'a [u8]) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move {
            self.0.write_all(buf).await?;
            self.0.flush().await
        })
    }
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
