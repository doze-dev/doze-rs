//! What the daemon runs beside serving (the module pool's reaper), and its stop: the engine's shutdown, in a hurry
//! when it is asked to stop a second time.

use crate::catalog::Catalog;
use std::sync::Arc;

/// Stops module processes nothing has called for a while: sleeps until the first could be due (the pool's idle limit
/// with none running), not on a fixed 30 s tick. The floor keeps a pass that stops nothing from repeating at once.
pub(super) async fn reap_modules(catalog: Arc<Catalog>) {
    const FLOOR: std::time::Duration = std::time::Duration::from_secs(1);
    loop {
        let wait = catalog.pool.next_reap().await.max(FLOOR);
        tokio::time::sleep(wait).await;
        catalog.pool.reap_idle().await;
    }
}

/// The engine's shutdown, dependants first, while the front stays open (on their way out they still reach what they
/// need through it). A second SIGTERM or SIGINT meanwhile (a second `doze down`, Ctrl-C twice) stops waiting for
/// dependants: whatever is not being stopped yet is stopped now, with a short grace.
#[allow(clippy::print_stderr)] // the daemon's stderr is its log file
pub(super) async fn shut_down(engine: &doze_engine::Engine) {
    let shutdown = engine.shutdown();
    tokio::pin!(shutdown);
    let Ok(again) = doze_os::signal::install() else {
        shutdown.await;
        return;
    };
    tokio::select! {
        () = &mut shutdown => return,
        _ = again.requested() => {}
    }
    eprintln!("doze daemon: asked to stop again: stopping everything still running now");
    engine.hurry();
    shutdown.await;
}
