//! The listening socket and the connection tasks.
//!
//! Each connection is served by hyper's HTTP/1 server on a task of its own, held to [`Limits`]: a header-read timeout (a client that
//! sends half a head, or keeps a connection open and sends nothing, is closed), and a cap on connections open at once (past it, the
//! next is accepted when one closes; the kernel's backlog holds it meanwhile). A failed accept (out of descriptors, say) waits a
//! moment before the next, so it cannot become a busy loop. With no connection the loop is parked on `accept`: it owns no timer.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use aws_wire::service::Service;
use doze_runtime::Tasks;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::Config;
use crate::pipeline::{Entry, Shared, respond};

/// How the gateway holds its connections.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Connections open at once.
    pub max_connections: usize,
    /// How long a connection may take to send a request's head, counted from when the server is ready for one: so it is also how
    /// long a kept-alive connection may sit idle before it is closed.
    pub header_timeout: Duration,
}

impl Default for Limits {
    /// 1024 connections (an SDK's pool holds 10 to 50), and a minute for a head (SDKs drop an idle connection sooner).
    fn default() -> Self {
        Self {
            max_connections: 1024,
            header_timeout: Duration::from_secs(60),
        }
    }
}

/// How long a failed accept waits before the next.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(50);

/// A gateway that has not been started.
pub struct Gateway {
    config: Config,
    services: Vec<Arc<dyn Service>>,
    limits: Limits,
}

impl std::fmt::Debug for Gateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gateway")
            .field("services", &self.services.len())
            .finish_non_exhaustive()
    }
}

impl Gateway {
    /// A gateway with no services.
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            config,
            services: Vec::new(),
            limits: Limits::default(),
        }
    }

    /// Holds connections to `limits` rather than [`Limits::default`] (a test makes them small).
    #[must_use]
    pub const fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Adds a service. Requests whose credential scope names its signing name go to it.
    #[must_use]
    pub fn with(mut self, service: Arc<dyn Service>) -> Self {
        self.services.push(service);
        self
    }

    /// The services added, in order (what `doze-aws compat` lists).
    #[must_use]
    pub fn services(&self) -> &[Arc<dyn Service>] {
        &self.services
    }

    /// Binds the socket and starts accepting.
    ///
    /// # Errors
    /// The address cannot be bound.
    pub async fn bind(self) -> io::Result<Running> {
        let listener = TcpListener::bind(self.config.listen).await?;
        let addr = listener.local_addr()?;
        let entries = self.services.into_iter().map(Entry::new).collect();
        let tasks = Tasks::new();
        let shared = Arc::new(Shared::new(entries, self.config, tasks.clone()));
        tasks.spawn(accept(listener, shared, tasks.clone(), self.limits));
        Ok(Running { addr, tasks })
    }
}

/// A running gateway.
#[derive(Debug)]
pub struct Running {
    addr: SocketAddr,
    tasks: Tasks,
}

impl Running {
    /// The address it listens on (the chosen port, if the configured one was 0).
    #[must_use]
    pub const fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stops accepting, ends every connection, and waits until they are gone.
    pub async fn shutdown(self) {
        self.tasks.shutdown().await;
    }
}

async fn accept(listener: TcpListener, shared: Arc<Shared>, tasks: Tasks, limits: Limits) {
    let stopping = tasks.stopping();
    let slots = Arc::new(Semaphore::new(limits.max_connections));
    // Said once until an accept succeeds again: a machine out of descriptors would otherwise log twenty lines a second.
    let mut told = false;
    loop {
        // A place first: at the cap the loop waits here, and the next connection waits in the kernel's backlog.
        let Some(Ok(slot)) = stopping
            .run_until_cancelled(Arc::clone(&slots).acquire_owned())
            .await
        else {
            return;
        };
        let Some(accepted) = stopping.run_until_cancelled(listener.accept()).await else {
            return;
        };
        let stream = match accepted {
            Ok((stream, _)) => stream,
            Err(e) => {
                if !told {
                    tracing::warn!("accept failed: {e} (trying again every {ACCEPT_BACKOFF:?})");
                    told = true;
                }
                drop(slot);
                if stopping
                    .run_until_cancelled(tokio::time::sleep(ACCEPT_BACKOFF))
                    .await
                    .is_none()
                {
                    return;
                }
                continue;
            }
        };
        told = false;
        let (shared, stopping) = (Arc::clone(&shared), stopping.clone());
        tasks.spawn(async move {
            connection(stream, shared, stopping, limits.header_timeout).await;
            drop(slot);
        });
    }
}

async fn connection(
    stream: tokio::net::TcpStream,
    shared: Arc<Shared>,
    stopping: CancellationToken,
    header_timeout: Duration,
) {
    // WHY: a small answer is written at once, not held back for the client's acknowledgement (Nagle against delayed ACKs).
    drop(stream.set_nodelay(true));
    let service = service_fn(move |req| {
        let shared = Arc::clone(&shared);
        async move {
            let response = respond(&shared, req).await;
            // A Lab rule that drops a request answers it with nothing: an error from the service function ends the connection.
            if crate::fault::is_dropped(&response) {
                return Err(crate::fault::Dropped);
            }
            Ok::<_, crate::fault::Dropped>(response)
        }
    });
    let mut builder = http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(header_timeout);
    // Upgrades are allowed so a service can take a connection over (a WebSocket API).
    let conn = builder
        .serve_connection(TokioIo::new(stream), service)
        .with_upgrades();
    // A stop ends the connection even if a request is in flight (as the Kafka server does).
    if let Some(Err(e)) = stopping.run_until_cancelled(conn).await {
        tracing::debug!("connection ended: {e}");
    }
}
