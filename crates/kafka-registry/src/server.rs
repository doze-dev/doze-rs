//! The registry on the network: HTTP/1 over TCP (or TLS with the broker's certificate), each request read whole up to its
//! limit and answered by the [`Api`] on a blocking thread (an answer may parse and compare schemas and write the store).
//!
//! The limits are the oracle's: a request's head must arrive within [`HEADER_TIMEOUT`] and the whole request within
//! [`READ_TIMEOUT`]; at most [`MAX_CONNECTIONS`] are served at once, and one past that is closed.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::header::{AUTHORIZATION, CONTENT_TYPE, HOST, HeaderValue, USER_AGENT, WWW_AUTHENTICATE};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;

use crate::api::{self, Api, Body, Request, Response};

pub const HEADER_TIMEOUT: Duration = Duration::from_secs(10);
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_CONNECTIONS: usize = 256;
/// How long a stop waits for the requests in hand.
pub const STOP_GRACE: Duration = Duration::from_secs(3);

/// TLS for the listener, when the broker has a certificate.
#[cfg(feature = "tls")]
pub type Tls = Option<tokio_rustls::TlsAcceptor>;
/// A build without TLS serves plain HTTP only.
#[cfg(not(feature = "tls"))]
pub type Tls = ();

/// A registry that is serving.
#[derive(Debug)]
pub struct Running {
    addr: SocketAddr,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl Running {
    /// Where it listens.
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stops taking connections, finishes the requests in hand, and drops what is left after [`STOP_GRACE`].
    pub async fn shutdown(self) {
        self.stop.cancel();
        let mut task = self.task;
        if tokio::time::timeout(STOP_GRACE, &mut task).await.is_err() {
            task.abort();
        }
    }
}

/// Serves `api` on a listener that is already bound.
///
/// # Errors
/// The listener has no local address.
pub fn start_on(listener: TcpListener, api: Arc<Api>, tls: Tls) -> io::Result<Running> {
    let addr = listener.local_addr()?;
    let stop = CancellationToken::new();
    let task = tokio::spawn(accept_loop(listener, api, tls, stop.clone()));
    Ok(Running { addr, stop, task })
}

async fn accept_loop(listener: TcpListener, api: Arc<Api>, tls: Tls, stop: CancellationToken) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let mut connections = JoinSet::new();
    loop {
        let accepted = tokio::select! {
            () = stop.cancelled() => break,
            accepted = listener.accept() => accepted,
            Some(_) = connections.join_next(), if !connections.is_empty() => continue,
        };
        let (stream, peer) = match accepted {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "the schema registry could not accept a connection");
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let Ok(slot) = Arc::clone(&slots).try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let _ = stream.set_nodelay(true);
        let (api, stop) = (Arc::clone(&api), stop.clone());
        #[cfg(feature = "tls")]
        let tls = tls.clone();
        #[cfg(not(feature = "tls"))]
        let () = tls;
        connections.spawn(async move {
            #[cfg(feature = "tls")]
            if let Some(acceptor) = tls {
                match tokio::time::timeout(HEADER_TIMEOUT, acceptor.accept(stream)).await {
                    Ok(Ok(s)) => serve_connection(s, peer, api, stop).await,
                    // A plain-HTTP client on the TLS port, a client that does not trust the certificate: its business.
                    _ => tracing::debug!(%peer, "a schema registry TLS handshake failed"),
                }
                drop(slot);
                return;
            }
            serve_connection(stream, peer, api, stop).await;
            drop(slot);
        });
    }
    while connections.join_next().await.is_some() {}
}

async fn serve_connection<S>(stream: S, peer: SocketAddr, api: Arc<Api>, stop: CancellationToken)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |req| answer(Arc::clone(&api), peer, req));
    let mut builder = http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_TIMEOUT);
    let conn = builder.serve_connection(TokioIo::new(stream), service);
    tokio::pin!(conn);
    let result = tokio::select! {
        r = conn.as_mut() => r,
        () = stop.cancelled() => {
            conn.as_mut().graceful_shutdown();
            conn.as_mut().await
        }
    };
    if let Err(e) = result {
        tracing::debug!(error = %e, "a schema registry connection ended with an error");
    }
}

fn header(req: &hyper::Request<Incoming>, name: &hyper::header::HeaderName) -> Option<String> {
    req.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// Reads a request whole (its body up to the path's limit) and answers it.
async fn answer(
    api: Arc<Api>,
    peer: SocketAddr,
    req: hyper::Request<Incoming>,
) -> Result<hyper::Response<Full<Bytes>>, std::convert::Infallible> {
    let limit = api::body_limit(req.uri().path());
    let mut r = Request::new(req.method().as_str(), "/");
    r.path = req.uri().path().to_owned();
    r.query = req.uri().query().unwrap_or_default().to_owned();
    r.host = header(&req, &HOST)
        .or_else(|| req.uri().authority().map(|a| a.to_string()))
        .unwrap_or_default();
    r.authorization = header(&req, &AUTHORIZATION);
    r.content_type = header(&req, &CONTENT_TYPE);
    r.user_agent = header(&req, &USER_AGENT).unwrap_or_default();
    r.peer = peer.ip().to_string();
    r.own = req.headers().contains_key(api::log::OWN_HEADER);
    let body =
        tokio::time::timeout(READ_TIMEOUT, Limited::new(req.into_body(), limit).collect()).await;
    r.body = match body {
        Ok(Ok(collected)) => Body::Read(collected.to_bytes().to_vec()),
        Ok(Err(e))
            if e.downcast_ref::<http_body_util::LengthLimitError>()
                .is_some() =>
        {
            Body::TooLarge(limit)
        }
        // A body that stopped arriving, or a broken connection: nothing to answer that anybody reads.
        _ => return Ok(plain(408, Vec::new())),
    };
    let resp = tokio::task::spawn_blocking(move || api.serve(&r)).await;
    let resp = resp.unwrap_or_else(|_| Response {
        status: 500,
        body: b"{\"error_code\":50001,\"message\":\"Internal error\"}\n".to_vec(),
        challenge: false,
        delay: Duration::ZERO,
    });
    if !resp.delay.is_zero() {
        tokio::time::sleep(resp.delay).await;
    }
    let mut out = plain(resp.status, resp.body);
    if resp.challenge {
        out.headers_mut().insert(
            WWW_AUTHENTICATE,
            HeaderValue::from_static("Basic realm=\"doze-kafka schema registry\""),
        );
    }
    Ok(out)
}

fn plain(status: u16, body: Vec<u8>) -> hyper::Response<Full<Bytes>> {
    let mut out = hyper::Response::new(Full::new(Bytes::from(body)));
    *out.status_mut() =
        hyper::StatusCode::from_u16(status).unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR);
    out.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(api::CONTENT_TYPE));
    out
}
