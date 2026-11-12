//! The server handle: start it, ask where it is, stop it.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use kafka_broker::Broker;
use kafka_storage::fs::Fs;
use kafka_wire::messages::ApiKey;
use kafka_wire::profile::Features;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::Config;
use crate::accept;
use crate::connection::Context;

/// Starts servers.
#[derive(Debug)]
pub struct Server;

impl Server {
    /// Binds `config.listen` and starts accepting connections for `broker`. The same call serves the binary
    /// and the tests.
    ///
    /// # Errors
    /// The address cannot be bound.
    pub async fn bind<F: Fs>(config: Config, broker: Arc<Broker<F>>) -> io::Result<Running> {
        let listener = TcpListener::bind(config.listen).await?;
        let addr = listener.local_addr()?;
        let ctx = Arc::new(Context {
            listener: config.advertise(addr),
            profile: config.profile,
            features: features(&config, &broker),
            broker,
            timeouts: config.timeouts,
            auth: config.sasl,
        });
        #[cfg(feature = "tls")]
        let tls = config.tls.as_ref().map(crate::TlsConfig::acceptor);
        #[cfg(not(feature = "tls"))]
        let tls = ();
        let limit = Arc::new(Semaphore::new(config.max_connections));
        let (stop, tasks) = (CancellationToken::new(), TaskTracker::new());
        tasks.spawn(accept::run(
            listener,
            ctx,
            limit,
            stop.clone(),
            tasks.clone(),
            tls,
        ));
        Ok(Running { addr, stop, tasks })
    }
}

/// What ApiVersions may list: the broker's APIs, and the two SASL ones only if the server has an authenticator to
/// answer them with.
fn features<F: Fs>(config: &Config, broker: &Broker<F>) -> Features {
    let sasl = config.sasl.is_some();
    let served = broker.served();
    Features {
        sasl,
        served: if sasl {
            served
                .with(ApiKey::SaslHandshake)
                .with(ApiKey::SaslAuthenticate)
        } else {
            served
        },
        ..Features::default()
    }
}

/// A server that is accepting connections.
#[derive(Debug)]
pub struct Running {
    addr: SocketAddr,
    stop: CancellationToken,
    tasks: TaskTracker,
}

impl Running {
    /// The address it is listening on (the port that was chosen, if the configured one was 0).
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stops accepting, ends every connection (a request being handled is dropped, not waited for) and waits
    /// until they are gone.
    pub async fn shutdown(self) {
        self.stop.cancel();
        self.tasks.close();
        self.tasks.wait().await;
    }
}
