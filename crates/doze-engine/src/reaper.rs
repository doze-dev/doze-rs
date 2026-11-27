//! The engine's background work: the idle reaper (Go's `RunReaper`) and the health prober of supervised commands
//! (Go's `RunHealthProber`, which records and never restarts: a blip must not cause a flap).
//!
//! Neither ticks while nothing happens. Go's reaper looked every second and its prober every 3 s, around the clock, so
//! an idle stack woke its daemon (and, through the probes, every app) for nothing. Here the reaper sleeps until the
//! first idle deadline or until the instances change; the prober looks every [`HEALTH_EVERY`] only while a running
//! command has a check to repeat, and otherwise waits for a change.

use crate::engine::Engine;
use crate::lifecycle::Now;
use doze_module_proto::msg::run::Ready;
use std::time::Duration;
use tokio::task::JoinHandle;

/// How often the health prober repeats the checks. Its answer is a record (nothing restarts on it), so it is taken
/// seldom: each probe is a request into the app as well as a wakeup of the daemon.
pub const HEALTH_EVERY: Duration = Duration::from_secs(30);

/// The least time between two reaper passes, so an instance that stays due (its stop was refused) cannot spin it.
const REAP_FLOOR: Duration = Duration::from_millis(250);

/// The background tasks of an engine; dropping this stops them (not the instances).
#[derive(Debug)]
pub struct Background {
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Background {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

impl Engine {
    /// Starts the reaper and the health prober.
    pub fn background(&self) -> Background {
        let reaper = tokio::spawn(reap_loop(self.clone()));
        let health = tokio::spawn(health_loop(self.clone()));
        Background {
            tasks: vec![reaper, health],
        }
    }

    /// One pass of the health prober (it runs every [`HEALTH_EVERY`] under [`Engine::background`]).
    pub async fn probe_health(&self) {
        probe_once(self).await;
    }

    /// One pass of the reaper: stops what has been idle for its time (decided under the lock, so a client that just
    /// connected is never cut off). Returns what it stopped.
    pub async fn reap_once(&self) -> Vec<String> {
        let fx = self.update(|c| c.m.reap(Now::current()));
        let slept: Vec<String> = fx
            .iter()
            .filter_map(|f| match f {
                crate::machine::Effect::StopUnit { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        for w in self.run(fx) {
            let _ = w.await;
        }
        for name in &slept {
            tracing::info!("\"{name}\" went to sleep: nothing held it for its idle time");
        }
        slept
    }
}

/// Sleeps until the first instance is due to sleep, reaps, and again; any change (a connection ending, a stop, a
/// start) recomputes the deadline. With nothing that could sleep it waits for a change and sets no timer at all; an
/// instance sleeps at its deadline, not up to a tick after it.
async fn reap_loop(e: Engine) {
    let mut changes = e.changes();
    let mut floor = tokio::time::Instant::now();
    loop {
        e.background_woke();
        let due = e.update(|c| c.m.next_reap());
        match due {
            Some(at) => {
                let at = tokio::time::Instant::from_std(at).max(floor);
                tokio::select! {
                    () = tokio::time::sleep_until(at) => {
                        e.reap_once().await;
                        floor = tokio::time::Instant::now() + REAP_FLOOR;
                    }
                    r = changes.changed() => if r.is_err() { return },
                }
            }
            None => {
                if changes.changed().await.is_err() {
                    return;
                }
            }
        }
    }
}

/// Probes, then waits [`HEALTH_EVERY`] while a running command has a check to repeat, or for a change while none has
/// (a command without one counts as healthy: marking it needs no timer).
async fn health_loop(e: Engine) {
    let mut changes = e.changes();
    loop {
        e.background_woke();
        let seen = running_commands(&e);
        if probe_once(&e).await {
            // Repeat in HEALTH_EVERY, or sooner when a command starts (or restarts) running: it is probed at once
            // rather than showing no answer for up to that long. Other changes (connections) do not cut the wait.
            let deadline = tokio::time::Instant::now() + HEALTH_EVERY;
            loop {
                tokio::select! {
                    () = tokio::time::sleep_until(deadline) => break,
                    r = changes.changed() => {
                        if r.is_err() {
                            return;
                        }
                        if running_commands(&e).iter().any(|n| !seen.contains(n)) {
                            break;
                        }
                    }
                }
            }
        } else if changes.changed().await.is_err() {
            return;
        }
    }
}

/// The supervised commands running now, each with its process: a restart is a new pid under the same name.
fn running_commands(e: &Engine) -> Vec<(String, u32)> {
    let now = Now::current();
    e.update(|c| {
        c.live
            .iter()
            .filter(|(_, l)| l.supervised)
            .filter_map(|(n, _)| c.m.table().get(n, now))
            .filter(|i| i.state.running())
            .map(|i| (i.name, i.pid))
            .collect()
    })
}

/// One health probe's limit (Go's).
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

/// Probes every running supervised command. With no check, or a one-shot `log_line` one, a running command counts as
/// healthy (Go's `CheckHealth` returned nil for those). Whether any of them had a check to repeat.
pub(crate) async fn probe_once(e: &Engine) -> bool {
    let checks: Vec<(String, Option<Ready>)> = e.update(|c| {
        c.live
            .iter()
            .filter(|(n, l)| l.supervised && c.m.table().state(n).is_some_and(|s| s.running()))
            .map(|(n, l)| {
                (
                    n.clone(),
                    l.health.clone().filter(|r| !matches!(r, Ready::LogLine(_))),
                )
            })
            .collect()
    });
    let any = checks.iter().any(|(_, r)| r.is_some());
    for (name, ready) in checks {
        let pids = e.pids(&name);
        let healthy = match ready {
            None => true,
            Some(r) => matches!(
                tokio::time::timeout(HEALTH_TIMEOUT, crate::ready::probe(&r, None, Some(&pids)))
                    .await,
                Ok(Ok(true))
            ),
        };
        e.update(|c| {
            if c.m.table().state(&name).is_some_and(|s| s.running()) {
                c.m.table_mut().set_healthy(&name, healthy);
            }
        });
    }
    any
}
