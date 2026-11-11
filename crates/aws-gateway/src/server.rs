//! The listening socket and the connection tasks.

use std::convert::Infallible;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use aws_schema::patterns::Patterns;
use aws_wire::service::Service;
use doze_runtime::Tasks;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::Config;
use crate::pipeline::{Entry, Shared, respond};

/// A gateway that has not been started.
pub struct Gateway {
    config: Config,
    services: Vec<Arc<dyn Service>>,
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
        }
    }

    /// Adds a service. Requests whose credential scope names its signing name go to it.
    #[must_use]
    pub fn with(mut self, service: Arc<dyn Service>) -> Self {
        self.services.push(service);
        self
    }

    /// Binds the socket and starts accepting.
    ///
    /// # Errors
    /// The address cannot be bound.
    pub async fn bind(self) -> io::Result<Running> {
        let listener = TcpListener::bind(self.config.listen).await?;
        let addr = listener.local_addr()?;
        let entries = self
            .services
            .into_iter()
            .map(|service| Entry {
                patterns: Patterns::new(service.tables()),
                service,
            })
            .collect();
        let shared = Arc::new(Shared::new(entries, self.config));
        let tasks = Tasks::new();
        tasks.spawn(accept(listener, shared, tasks.clone()));
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

async fn accept(listener: TcpListener, shared: Arc<Shared>, tasks: Tasks) {
    let stopping = tasks.stopping();
    loop {
        let Some(accepted) = stopping.run_until_cancelled(listener.accept()).await else {
            return;
        };
        let stream = match accepted {
            Ok((stream, _)) => stream,
            Err(e) => {
                tracing::warn!("accept failed: {e}");
                continue;
            }
        };
        tasks.spawn(connection(stream, Arc::clone(&shared), stopping.clone()));
    }
}

async fn connection(
    stream: tokio::net::TcpStream,
    shared: Arc<Shared>,
    stopping: CancellationToken,
) {
    let service = service_fn(move |req| {
        let shared = Arc::clone(&shared);
        async move { Ok::<_, Infallible>(respond(&shared, req).await) }
    });
    let conn = http1::Builder::new().serve_connection(TokioIo::new(stream), service);
    // A stop ends the connection even if a request is in flight (as the Kafka server does).
    if let Some(Err(e)) = stopping.run_until_cancelled(conn).await {
        tracing::debug!("connection ended: {e}");
    }
}
