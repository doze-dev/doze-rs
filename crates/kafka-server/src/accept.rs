//! Taking connections.

use std::sync::Arc;
use std::time::Duration;

use kafka_storage::fs::Fs;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio::time::{Instant, sleep};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{debug, warn};

use crate::connection::{self, Context, End};

/// What the listener wraps a connection in: the acceptor of a build with TLS (none, if no certificate was given), and
/// nothing at all in a build without.
#[cfg(feature = "tls")]
pub(crate) type Tls = Option<crate::tls::TlsAcceptor>;
#[cfg(not(feature = "tls"))]
pub(crate) type Tls = ();

/// A TLS connection's close_notify is given this long to be written before the socket is dropped regardless.
#[cfg(feature = "tls")]
const CLOSE_NOTIFY: Duration = Duration::from_secs(1);

/// Accepts connections until `stop`, each served on its own task of `tasks`. A connection past the limit is
/// closed as it arrives.
pub(crate) async fn run<F: Fs>(
    listener: TcpListener,
    ctx: Arc<Context<F>>,
    limit: Arc<Semaphore>,
    stop: CancellationToken,
    tasks: TaskTracker,
    tls: Tls,
) {
    // Shared by the connection tasks (a build without TLS shares a `()`).
    let tls = Arc::new(tls);
    let mut backoff = Duration::ZERO;
    let mut refused = 0u64;
    loop {
        let accepted = tokio::select! {
            a = listener.accept() => a,
            () = stop.cancelled() => return,
        };
        let (socket, peer) = match accepted {
            Ok(accepted) => accepted,
            Err(e) => {
                // Out of descriptors, say: wait for one to come back rather than spin.
                backoff = (backoff * 2).clamp(Duration::from_millis(5), Duration::from_secs(1));
                warn!(error = %e, retry_in = ?backoff, "accepting a connection failed");
                tokio::select! {
                    () = sleep(backoff) => {}
                    () = stop.cancelled() => return,
                }
                continue;
            }
        };
        backoff = Duration::ZERO;
        let Ok(permit) = Arc::clone(&limit).try_acquire_owned() else {
            refused += 1;
            if refused == 1 || refused.is_multiple_of(1000) {
                warn!(
                    refused_so_far = refused,
                    "refusing connections: at the connection limit"
                );
            }
            continue;
        };
        // Requests are small and answered one at a time: waiting to coalesce them only adds latency.
        let _ = socket.set_nodelay(true);
        let (ctx, stop, tls) = (Arc::clone(&ctx), stop.clone(), Arc::clone(&tls));
        let arrived = Instant::now();
        // Everything from here, the TLS handshake included, is the connection's own task: a client that is slow to
        // handshake holds a place in the limit and nothing else. The peer's address is the socket's, taken before.
        tasks.spawn(async move {
            let _permit = permit;
            let end = serve_socket(socket, (peer, arrived), &ctx, &tls, &stop).await;
            debug!(%peer, %end, "connection closed");
        });
    }
}

/// One accepted socket, served in the clear.
#[cfg(not(feature = "tls"))]
async fn serve_socket<F: Fs>(
    mut socket: TcpStream,
    (peer, arrived): (std::net::SocketAddr, Instant),
    ctx: &Context<F>,
    (): &Tls,
    stop: &CancellationToken,
) -> End {
    connection::serve(&mut socket, ctx, Some(peer), arrived, stop).await
}

/// One accepted socket: after a TLS handshake if the server has a certificate, else in the clear. The handshake has the
/// connection's pre-auth budget, SASL or not, and the login that follows has what is left of it.
#[cfg(feature = "tls")]
async fn serve_socket<F: Fs>(
    mut socket: TcpStream,
    (peer, arrived): (std::net::SocketAddr, Instant),
    ctx: &Context<F>,
    tls: &Tls,
    stop: &CancellationToken,
) -> End {
    use tokio::io::AsyncWriteExt;
    let Some(acceptor) = tls else {
        return connection::serve(&mut socket, ctx, Some(peer), arrived, stop).await;
    };
    let by = arrived + ctx.timeouts.pre_auth;
    let mut stream = match crate::tls::accept(acceptor, socket, by, stop).await {
        Ok(stream) => stream,
        Err(end) => return end,
    };
    let end = connection::serve(&mut stream, ctx, Some(peer), arrived, stop).await;
    // Say goodbye the TLS way (a close_notify) so a client reads a clean end and not a truncation; but a client that
    // has stopped reading does not hold a stop.
    let _ = tokio::time::timeout(CLOSE_NOTIFY, stream.shutdown()).await;
    end
}
