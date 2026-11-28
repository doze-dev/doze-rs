//! What the daemon runs beside serving (the module pool's reaper), and its stop: the engine's shutdown, in a hurry
//! when it is asked to stop a second time, and the lines it says on the way.

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

/// A line in the daemon's log (its stderr): one write, and no panic when it fails (a full disk), where `eprintln!`
/// would panic in the middle of a stop.
pub(super) fn say(line: &str) {
    use std::io::Write as _;
    let _ = std::io::stderr()
        .lock()
        .write_all(format!("{line}\n").as_bytes());
}

/// The engine's shutdown, dependants first, while the front stays open (on their way out they still reach what they
/// need through it). Another SIGTERM or SIGINT on the same handlers that saw the first (installed before the daemon
/// served, so none can fall between two installs) stops waiting for dependants: whatever is not being stopped yet is
/// stopped now in a hurry, and what is still being stopped 2 s later is killed.
pub(super) async fn shut_down(engine: &doze_engine::Engine, mut stop: doze_os::signal::Stop) {
    let shutdown = engine.shutdown();
    tokio::pin!(shutdown);
    // Biased, the shutdown first: a second request already pending when this is first polled must not hurry before
    // the shutdown has made its own first stops.
    tokio::select! {
        biased;
        () = &mut shutdown => return,
        _ = stop.next() => {}
    }
    say("doze daemon: asked to stop again: stopping everything still running now");
    engine.hurry();
    shutdown.await;
}
