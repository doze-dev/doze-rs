//! Serving the console on a listener and stopping it, and the connection loop that holds it to its limits. (doze-kafka's console
//! has the same loop; it belongs in a crate both share.)
//!
//! Each accepted connection is served by hyper's HTTP/1 server with a header-read timeout (a client that sends half a head,
//! or sends nothing, is closed), under a cap on connections open at once. A connection over the cap is answered 503 with
//! `Retry-After` and closed. Event streams have a cap of their own (in the state). The router is the one in [`crate::app`].
//!
//! With no connection the loop is parked on `accept`: it owns no timer.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::middleware::{from_fn, from_fn_with_state};
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;

use crate::app::console;
use crate::facts::Facts;
use crate::headers::security_headers;
use crate::limits::{Limits, refusal_bytes, request_timeout};
use crate::ring::Recorder;
use crate::state::{Shared, State};

/// The longest a stop waits for the server to wind down before it gives up on what is left.
pub const STOP_GRACE: Duration = Duration::from_secs(3);
/// Connections over the cap that are being answered at once; past this they are closed without an answer.
const REFUSALS_AT_ONCE: usize = 32;
/// How long a refusal is given to be written and read past.
const REFUSAL_TIME: Duration = Duration::from_millis(500);

/// How the console is served.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    /// `--console-allow-host`.
    pub allow_hosts: Vec<String>,
}

/// A console that is serving.
#[derive(Debug)]
pub struct Running {
    addr: SocketAddr,
    stop: CancellationToken,
    task: JoinHandle<()>,
    state: Shared,
}

/// Serves the console on a listener that is already bound (the binary binds first, so that an address that cannot be had stops
/// the start before anything is opened), answering from `facts` and the calls in `recorder`, until [`Running::shutdown`]. The
/// limits are [`Limits::default`].
///
/// # Errors
/// The listener has no local address.
pub fn start_on(
    listener: TcpListener,
    facts: Facts,
    recorder: Arc<Recorder>,
    settings: Settings,
) -> io::Result<Running> {
    start_with(listener, facts, recorder, settings, Limits::default())
}

/// [`start_on`] with the limits given (a test makes them small).
///
/// # Errors
/// The listener has no local address.
pub fn start_with(
    listener: TcpListener,
    facts: Facts,
    recorder: Arc<Recorder>,
    settings: Settings,
    limits: Limits,
) -> io::Result<Running> {
    let addr = listener.local_addr()?;
    let stop = CancellationToken::new();
    let state = State::new(
        facts,
        recorder,
        addr,
        settings.allow_hosts,
        stop.clone(),
        &limits,
    );
    let task = tokio::spawn(accept_loop(listener, state.clone(), stop.clone(), limits));
    Ok(Running {
        addr,
        stop,
        task,
        state,
    })
}

/// The router with the layers outside it. The timeout is outside the router's layers, so its 408 is given the security
/// headers by the layer over it.
pub fn app(state: Shared, limits: Limits) -> axum::Router {
    console(state)
        .layer(from_fn_with_state(limits.request_timeout, request_timeout))
        .layer(from_fn(security_headers))
}

/// Accepts until `stop`, serving each connection on a task of its own; then waits for them to wind down.
///
/// The router is built when the first connection is accepted, not before: axum boxes each route's service and its layers
/// (about 7 KB of heap a route, measured on doze-kafka's console), held for as long as the console runs, and a doze-aws whose
/// console is never opened should not pay it at idle.
async fn accept_loop(
    listener: TcpListener,
    state: Shared,
    stop: CancellationToken,
    limits: Limits,
) {
    let slots = Arc::new(Semaphore::new(limits.max_connections));
    let refusals = Arc::new(Semaphore::new(REFUSALS_AT_ONCE));
    let mut connections = JoinSet::new();
    let mut router: Option<axum::Router> = None;
    loop {
        let accepted = tokio::select! {
            () = stop.cancelled() => break,
            accepted = listener.accept() => accepted,
            // Reap finished connections as we go, so the set does not grow with every connection ever made.
            Some(_) = connections.join_next(), if !connections.is_empty() => continue,
        };
        let stream = match accepted {
            Ok((stream, _peer)) => stream,
            Err(e) => {
                // Out of descriptors, or a connection that was reset before it was accepted: neither ends the console.
                tracing::warn!(error = %e, "the console could not accept a connection");
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        match Arc::clone(&slots).try_acquire_owned() {
            Ok(slot) => {
                let service = TowerToHyperService::new(
                    router
                        .get_or_insert_with(|| app(state.clone(), limits))
                        .clone(),
                );
                let stop = stop.clone();
                connections.spawn(async move {
                    serve_connection(stream, service, stop, limits).await;
                    drop(slot);
                });
            }
            Err(_) => match Arc::clone(&refusals).try_acquire_owned() {
                Ok(turn) => {
                    connections.spawn(async move {
                        refuse(stream).await;
                        drop(turn);
                    });
                }
                // Too many refusals in hand as well: close without an answer.
                Err(_) => drop(stream),
            },
        }
    }
    // Stopping: the streams end with the token, the requests in hand are finished; whatever is left is dropped by the
    // caller after its grace.
    while connections.join_next().await.is_some() {}
}

async fn serve_connection(
    stream: TcpStream,
    service: TowerToHyperService<axum::Router>,
    stop: CancellationToken,
    limits: Limits,
) {
    let _ = stream.set_nodelay(true);
    let mut builder = http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(limits.header_timeout);
    let conn = builder.serve_connection(TokioIo::new(stream), service);
    tokio::pin!(conn);
    let result = tokio::select! {
        r = conn.as_mut() => r,
        () = stop.cancelled() => {
            // Finish the request in hand, take no more on this connection.
            conn.as_mut().graceful_shutdown();
            conn.as_mut().await
        }
    };
    if let Err(e) = result {
        tracing::debug!(error = %e, "a console connection ended with an error");
    }
}

/// Answers a connection that is over the cap with a 503 and closes it. The client's request is read past (briefly), so that
/// closing does not turn into a reset that throws the answer away.
async fn refuse(mut stream: TcpStream) {
    let answer = async {
        stream.write_all(&refusal_bytes()).await?;
        stream.shutdown().await?;
        let mut sink = [0_u8; 1024];
        while stream.read(&mut sink).await? > 0 {}
        io::Result::Ok(())
    };
    let _ = tokio::time::timeout(REFUSAL_TIME, answer).await;
}

impl Running {
    /// Where it listens (the port that was chosen, if the configured one was 0).
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// The URL a browser on this machine opens.
    #[must_use]
    pub fn url(&self) -> String {
        self.state.url()
    }

    /// The shared state, for the tests.
    #[must_use]
    pub fn state(&self) -> &Shared {
        &self.state
    }

    /// Stops: every open stream ends, the server stops taking connections and finishes the requests in hand, and
    /// whatever is still open after [`STOP_GRACE`] is dropped.
    pub async fn shutdown(self) {
        self.stop.cancel();
        let mut task = self.task;
        if tokio::time::timeout(STOP_GRACE, &mut task).await.is_err() {
            task.abort();
        }
    }
}
