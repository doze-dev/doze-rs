//! Before the stack's apps start: while the system still says a name this daemon holds does not exist (a negative
//! answer it cached while the stack was down), the daemon says so and waits a bounded time for it to clear, so an app
//! that looks the name up at boot does not crash with ENOTFOUND (`doze_names::settle` has the rules). Nothing when
//! names are off or the system does not ask this registry's zone. Once a look has run its course (every name resolved,
//! or the budget spent and said once with the fix), not again for this daemon: a name the system never resolves (a VPN
//! or split DNS owning .doze, a stale /etc/resolver/doze) costs one wait, not one per `up`. A restart is a new
//! daemon, which is when a stale answer bites. Two `up`s at once wait on the same look.

use doze_names::Registry;
use std::sync::OnceLock;

/// The registry this daemon's names are in, once it serves them.
static SERVED: OnceLock<Registry> = OnceLock::new();
/// Whether a look has run its course for this daemon; held while one is in progress, so a second `up` waits for it
/// rather than going on early.
static SETTLED: tokio::sync::Mutex<bool> = tokio::sync::Mutex::const_new(false);

/// Called when the daemon serves its names (names on).
pub(crate) fn serving(reg: &Registry) {
    let _ = SERVED.set(reg.clone());
}

/// Waits (bounded) while the system refuses one of this daemon's names; logs what it waited for.
pub(crate) async fn settle() {
    let Some(reg) = SERVED.get().cloned() else {
        return;
    };
    let mut settled = SETTLED.lock().await;
    if *settled {
        return;
    }
    let pid = i64::from(std::process::id());
    *settled = tokio::task::spawn_blocking(move || look(&reg, pid))
        .await
        .unwrap_or(false);
}

/// One look, and the wait: whether it ran its course (every name of `pid`'s resolved, or the budget was spent and
/// said). A machine whose system does not ask this zone, or a daemon with no names yet, has not: the next `up` looks
/// again (cheaply, with nothing to wait for).
fn look(reg: &Registry, pid: i64) -> bool {
    let hosts: Vec<String> = reg
        .snapshot()
        .into_iter()
        .filter(|(h, e)| e.pid == pid && !e.ip.is_empty() && doze_names::in_zone(h))
        .map(|(h, _)| h)
        .collect();
    if hosts.is_empty() || !doze_names::settle::system_asks_here(reg) {
        return false;
    }
    #[allow(clippy::print_stderr)] // the daemon's stderr is its log file
    let log = |l: String| eprintln!("doze daemon: {l}");
    // What is still refused after the budget was said in the log, with how to clear it; not waited for again.
    let _ = doze_names::settle::wait_resolved(
        &hosts,
        (doze_names::settle::BUDGET, doze_names::settle::EVERY),
        &doze_names::settle::system_resolves,
        &log,
    );
    true
}
