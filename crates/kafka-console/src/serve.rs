//! Binding the console and stopping it.

use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::app::router;
use crate::base::Base;
use crate::source::Source;
use crate::state::State;

/// The longest a stop waits for the server to wind down before it gives up on what is left.
pub const STOP_GRACE: Duration = Duration::from_secs(3);

/// How the console is served.
#[derive(Debug, Clone)]
pub struct Settings {
    pub addr: SocketAddr,
    /// `--console-prefix`.
    pub prefix: String,
    /// `--console-allow-host`.
    pub allow_hosts: Vec<String>,
}

/// A console that is serving.
#[derive(Debug)]
pub struct Running {
    addr: SocketAddr,
    stop: CancellationToken,
    task: JoinHandle<()>,
    state: crate::state::Shared,
}

/// Binds `settings.addr` and serves `source` there until [`Running::shutdown`].
///
/// # Errors
/// The address cannot be bound (in use, not an address of this machine).
pub async fn start(source: Source, settings: Settings) -> io::Result<Running> {
    let listener = TcpListener::bind(settings.addr).await?;
    start_on(listener, source, settings)
}

/// Serves `source` on a listener that is already bound (the binary binds first, so that an address that cannot be had
/// stops the start before anything is opened). `settings.addr` is not read.
///
/// # Errors
/// The listener has no local address.
pub fn start_on(listener: TcpListener, source: Source, settings: Settings) -> io::Result<Running> {
    let addr = listener.local_addr()?;
    let stop = CancellationToken::new();
    let state = State::new(
        source,
        Base::new(&settings.prefix),
        settings.allow_hosts,
        stop.clone(),
    );
    let app = router(state.clone());
    let wind_down = stop.clone();
    let task = tokio::spawn(async move {
        let served = axum::serve(listener, app)
            .with_graceful_shutdown(async move { wind_down.cancelled().await });
        if let Err(e) = served.await {
            tracing::warn!(error = %e, "the console stopped serving");
        }
    });
    Ok(Running {
        addr,
        stop,
        task,
        state,
    })
}

impl Running {
    /// Where it listens (the port that was chosen, if the configured one was 0).
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// The shared state, for tests that look at the stream's ticker.
    #[must_use]
    pub fn state(&self) -> &crate::state::Shared {
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
