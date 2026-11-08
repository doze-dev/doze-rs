//! Taking connections.

use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{debug, warn};

use crate::connection::{self, Context};

/// Accepts connections until `stop`, each served on its own task of `tasks`. A connection past the limit is
/// closed as it arrives.
pub(crate) async fn run(
    listener: TcpListener,
    ctx: Arc<Context>,
    limit: Arc<Semaphore>,
    stop: CancellationToken,
    tasks: TaskTracker,
) {
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
        let (ctx, stop) = (Arc::clone(&ctx), stop.clone());
        tasks.spawn(async move {
            let _permit = permit;
            let end = connection::serve(socket, &ctx, &stop).await;
            debug!(%peer, %end, "connection closed");
        });
    }
}
