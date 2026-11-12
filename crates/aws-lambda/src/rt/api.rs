//! The Lambda Runtime API, served on a loopback port of its own for each function process: the four routes every runtime speaks
//! (`next`, `response`, `error`, `init/error`), and the streaming form of `response`.
//!
//! A process polls `next` and is given the work its worker holds for it; it answers on `response` or `error`, and what it says goes
//! to whoever is waiting for the invocation over a channel of [`Chunk`]s.

use std::convert::Infallible;
use std::sync::{Arc, Mutex, PoisonError};

use bytes::Bytes;
use http::{Method, Request, Response, StatusCode};
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// The largest response a function may return (Lambda: 6 MiB plus 36 bytes of framing).
pub(crate) const MAX_RESPONSE: usize = 6 * 1024 * 1024 + 36;
const PREFIX: &str = "/2018-06-01/runtime/";

/// What a process said about an invocation.
#[derive(Debug)]
pub(crate) enum Chunk {
    /// The `Content-Type` the function gave its response (first, when it gave one).
    Type(String),
    /// A piece of the response.
    Data(Bytes),
    /// The response is complete.
    End,
    /// The invocation failed: the error document the runtime posted.
    Fail(Bytes),
}

/// One invocation, handed to the process that polls for it.
#[derive(Debug)]
pub(crate) struct Work {
    pub(crate) id: String,
    pub(crate) payload: Bytes,
    pub(crate) deadline_ms: i64,
    pub(crate) invoked_arn: String,
    pub(crate) trace_id: String,
    pub(crate) client_context: Option<String>,
    /// Sent when the process fetches the work: the moment the function's own timeout starts.
    pub(crate) started: oneshot::Sender<()>,
    pub(crate) sink: mpsc::Sender<Chunk>,
}

/// What the API server and the worker share.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) inbox: AsyncMutex<mpsc::Receiver<Work>>,
    current: Mutex<Option<(String, mpsc::Sender<Chunk>)>>,
    /// Set when the process reported that it failed to initialise.
    pub(crate) init_error: Mutex<Option<Bytes>>,
}

impl Shared {
    pub(crate) fn new(inbox: mpsc::Receiver<Work>) -> Self {
        Self { inbox: AsyncMutex::new(inbox), current: Mutex::new(None), init_error: Mutex::new(None) }
    }

    /// Forgets the invocation in flight, which closes its channel: whoever waits sees the process went away.
    pub(crate) fn drop_current(&self) {
        self.current.lock().unwrap_or_else(PoisonError::into_inner).take();
    }

    fn set_current(&self, id: String, sink: mpsc::Sender<Chunk>) {
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) = Some((id, sink));
    }

    fn sink_of(&self, id: &str) -> Option<mpsc::Sender<Chunk>> {
        let current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        current.as_ref().filter(|(i, _)| i == id).map(|(_, s)| s.clone())
    }

    fn finish(&self, id: &str) {
        let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        if current.as_ref().is_some_and(|(i, _)| i == id) {
            current.take();
        }
    }
}

/// Accepts connections until `stop` is cancelled.
pub(crate) async fn serve(listener: TcpListener, shared: Arc<Shared>, stop: CancellationToken, tasks: doze_runtime::Tasks) {
    loop {
        let Some(Ok((stream, _))) = stop.run_until_cancelled(listener.accept()).await else { return };
        let (shared, stop) = (Arc::clone(&shared), stop.clone());
        tasks.spawn(async move {
            let service = service_fn(move |req| {
                let shared = Arc::clone(&shared);
                async move { Ok::<_, Infallible>(route(&shared, req).await) }
            });
            let conn = http1::Builder::new().title_case_headers(true).serve_connection(TokioIo::new(stream), service);
            drop(stop.run_until_cancelled(conn).await);
        });
    }
}

fn reply(status: StatusCode, body: &'static str) -> Response<Full<Bytes>> {
    let mut r = Response::new(Full::new(Bytes::from_static(body.as_bytes())));
    *r.status_mut() = status;
    r
}

async fn route(shared: &Shared, req: Request<Incoming>) -> Response<Full<Bytes>> {
    let path = req.uri().path().to_owned();
    let Some(rest) = path.strip_prefix(PREFIX) else {
        return reply(StatusCode::NOT_FOUND, "{\"errorType\":\"NotFound\"}");
    };
    let parts: Vec<&str> = rest.split('/').collect();
    match (req.method().clone(), parts.as_slice()) {
        (Method::GET, ["invocation", "next"]) => next(shared).await,
        (Method::POST, ["invocation", id, "response"]) => response(shared, id, req).await,
        (Method::POST, ["invocation", id, "error"]) => error(shared, id, req).await,
        (Method::POST, ["init", "error"]) => {
            let body = collect(req.into_body(), 1 << 20).await.unwrap_or_default();
            *shared.init_error.lock().unwrap_or_else(PoisonError::into_inner) = Some(body);
            reply(StatusCode::ACCEPTED, "{\"status\":\"OK\"}")
        }
        _ => reply(StatusCode::NOT_FOUND, "{\"errorType\":\"NotFound\"}"),
    }
}

async fn next(shared: &Shared) -> Response<Full<Bytes>> {
    let Some(work) = shared.inbox.lock().await.recv().await else {
        return reply(StatusCode::GONE, "{\"errorType\":\"Gone\"}");
    };
    shared.set_current(work.id.clone(), work.sink.clone());
    // The receiver may have given up (the caller went away); the work is run anyway, as Lambda would.
    drop(work.started.send(()));
    let mut r = Response::new(Full::new(work.payload));
    let mut set = |name: &'static str, value: &str| {
        if let Ok(v) = http::HeaderValue::from_str(value) {
            r.headers_mut().insert(name, v);
        }
    };
    set("Lambda-Runtime-Aws-Request-Id", &work.id);
    set("Lambda-Runtime-Deadline-Ms", &work.deadline_ms.to_string());
    set("Lambda-Runtime-Invoked-Function-Arn", &work.invoked_arn);
    set("Lambda-Runtime-Trace-Id", &work.trace_id);
    if let Some(cc) = &work.client_context {
        set("Lambda-Runtime-Client-Context", cc);
    }
    r.headers_mut().insert("content-type", http::HeaderValue::from_static("application/json"));
    r
}

/// The whole body, up to `limit` bytes; `None` when it is longer.
async fn collect(body: Incoming, limit: usize) -> Option<Bytes> {
    Limited::new(body, limit).collect().await.ok().map(http_body_util::Collected::to_bytes)
}

async fn response(shared: &Shared, id: &str, req: Request<Incoming>) -> Response<Full<Bytes>> {
    let Some(sink) = shared.sink_of(id) else {
        return reply(StatusCode::BAD_REQUEST, "{\"errorType\":\"InvalidStateTransition\"}");
    };
    let streaming = req
        .headers()
        .get("lambda-runtime-function-response-mode")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("streaming"));
    if let Some(t) = req.headers().get("content-type").and_then(|v| v.to_str().ok()) {
        drop(sink.send(Chunk::Type(t.to_owned())).await);
    }
    if streaming {
        let mut body = req.into_body();
        while let Some(frame) = body.frame().await {
            match frame {
                Ok(f) => {
                    if let Ok(data) = f.into_data() {
                        drop(sink.send(Chunk::Data(data)).await);
                    }
                }
                Err(_) => {
                    drop(sink.send(Chunk::Fail(Bytes::from_static(b"{\"errorType\":\"Runtime.StreamError\",\"errorMessage\":\"The response stream broke.\"}"))).await);
                    shared.finish(id);
                    return reply(StatusCode::ACCEPTED, "{\"status\":\"OK\"}");
                }
            }
        }
        drop(sink.send(Chunk::End).await);
    } else {
        match collect(req.into_body(), MAX_RESPONSE).await {
            Some(bytes) => {
                drop(sink.send(Chunk::Data(bytes)).await);
                drop(sink.send(Chunk::End).await);
            }
            None => {
                let msg = format!("{{\"errorType\":\"Function.ResponseSizeTooLarge\",\"errorMessage\":\"Response payload size exceeded maximum allowed payload size ({MAX_RESPONSE} bytes).\"}}");
                drop(sink.send(Chunk::Fail(Bytes::from(msg))).await);
            }
        }
    }
    shared.finish(id);
    reply(StatusCode::ACCEPTED, "{\"status\":\"OK\"}")
}

async fn error(shared: &Shared, id: &str, req: Request<Incoming>) -> Response<Full<Bytes>> {
    let Some(sink) = shared.sink_of(id) else {
        return reply(StatusCode::BAD_REQUEST, "{\"errorType\":\"InvalidStateTransition\"}");
    };
    let body = collect(req.into_body(), 1 << 20).await.unwrap_or_default();
    drop(sink.send(Chunk::Fail(body)).await);
    shared.finish(id);
    reply(StatusCode::ACCEPTED, "{\"status\":\"OK\"}")
}
