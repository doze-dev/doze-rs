//! Stopping, the crash watcher and restart timers: the shell side of the machine's stop, exit and restart decisions.
//! A stop runs as its own task, so a caller that gives up waiting (a dropped future, an aborted reaper) never leaves
//! a process running that nothing tracks: the unit stays the machine's (as stopping) until the task reports it gone.

use crate::engine::Engine;
use crate::error::Error;
use crate::events::Cause;
use crate::exec::{STOP_GRACE, Why};
use crate::lifecycle::Now;
use crate::steps::{self, Moment, STOP_PATIENCE};
use crate::supervise::{Ended, Handle};
use std::time::Duration;
use tokio::task::JoinHandle;

/// How long the shutdown stops things in order (dependants first, each with its grace), all waves together. Past it,
/// what is being stopped is killed and the rest stopped at once ([`Engine::shutdown_within`]): a stuck stop at the top
/// of a chain must not keep the shutdown, or the databases at the bottom of it, waiting for ever.
pub(crate) const SHUTDOWN_LIMIT: Duration = Duration::from_secs(120);
/// The grace of a stop made in a hurry ([`Engine::hurry`], or the shutdown out of time): no stop steps, no patience,
/// and this long after the signal before the kill.
pub(crate) const HURRY_GRACE: Duration = Duration::from_secs(2);
/// After the deadline, how long the hurried stops get (their 2 s grace; an interrupt is SIGINT for that, then SIGQUIT
/// for 3 s) before whatever is left is killed.
const HURRY_WAIT: Duration = Duration::from_secs(6);
/// How long the last kills get to be reaped and said.
const REAP_WAIT: Duration = Duration::from_secs(1);

impl Engine {
    /// Stops `name` if it runs, cancels a start in flight and a pending restart, and releases the needs it held once
    /// it is gone. An intentional stop resets the restart budget, so nothing respawns behind it.
    pub async fn stop(&self, name: &str, cause: Cause) -> Result<(), Error> {
        let fx = self.update(|c| c.m.stop(name, cause))?;
        for w in self.run(fx) {
            let _ = w.await;
        }
        Ok(())
    }

    /// Stops everything (cancelling starts and restarts) and waits, within the shutdown limit (120 s), until nothing
    /// runs.
    pub async fn stop_all(&self, cause: Cause) {
        let deadline = tokio::time::Instant::now() + SHUTDOWN_LIMIT;
        let fx = self.update(|c| c.m.stop_all(cause.clone()));
        for w in self.run(fx) {
            let _ = tokio::time::timeout_at(deadline, w).await;
        }
        if !self.wait_quiet(cause, deadline).await {
            tracing::warn!("stopping took longer than {SHUTDOWN_LIMIT:?}; giving up waiting");
        }
    }

    /// Stops for good: no start and no restart from now on (a start in flight is stopped as soon as it registers),
    /// then everything is stopped, dependants first, within the shutdown limit (120 s). One way.
    pub async fn shutdown(&self) {
        self.shutdown_within(SHUTDOWN_LIMIT).await;
    }

    /// [`Engine::shutdown`] with `limit` for the whole ordered part, every wave included. When it runs out, what is
    /// being stopped is killed, what still waits for its dependants is stopped now in a hurry (no stop steps, a 2 s
    /// grace), and whatever is still there 6 s later is killed; each kill is said, in the log and the journal, before
    /// this returns (7 s after the deadline at most): nothing is left for the daemon's exit to kill unsaid or orphan.
    pub async fn shutdown_within(&self, limit: Duration) {
        let deadline = tokio::time::Instant::now() + limit;
        let ordered = async {
            let fx = self.update(|c| c.m.shutdown());
            for w in self.run(fx) {
                let _ = w.await;
            }
            self.wait_quiet(Cause::Shutdown, deadline).await
        };
        if let Ok(true) = tokio::time::timeout_at(deadline, ordered).await {
            return;
        }
        tracing::warn!(
            "stopping in order took longer than {}s; killing what is still being stopped, stopping the rest now",
            limit.as_secs()
        );
        // What was being stopped has had its time: killed. What was still waiting for its dependants is stopped now,
        // in a hurry.
        let stuck = self.stopping_handles();
        for h in &stuck {
            h.kill(Why::OutOfTime);
        }
        let fx = self.update(|c| c.m.hurry());
        let waits = self.run(fx);
        let until = tokio::time::Instant::now() + HURRY_WAIT;
        for w in waits {
            let _ = tokio::time::timeout_at(until, w).await;
        }
        if self.wait_quiet(Cause::Shutdown, until).await {
            return;
        }
        // Still there: killed now, and given a moment to be reaped, so each kill is said rather than done unsaid by
        // the daemon's exit.
        for h in self.update(|c| c.handles.values().cloned().collect::<Vec<Handle>>()) {
            h.kill(Why::OutOfTime);
        }
        if !self
            .wait_quiet(Cause::Shutdown, tokio::time::Instant::now() + REAP_WAIT)
            .await
        {
            tracing::warn!("the shutdown's kills had not all been reaped {REAP_WAIT:?} later");
        }
    }

    /// Asked to stop again while stopping: no more waiting for dependants. Whatever is not being stopped yet is stopped
    /// now in a hurry (no stop steps, a 2 s grace), and whatever is still being stopped 2 s from now is killed.
    pub fn hurry(&self) {
        let fx = self.update(|c| c.m.hurry());
        drop(self.run(fx));
        // After the hurry: the stops it just made are on the list too.
        let stopping = self.stopping_handles();
        tokio::spawn(async move {
            tokio::time::sleep(HURRY_GRACE).await;
            for h in &stopping {
                h.kill(Why::AskedAgain);
            }
        });
    }

    /// The handles of the units being stopped.
    fn stopping_handles(&self) -> Vec<Handle> {
        self.update(|c| {
            c.m.stopping_units()
                .iter()
                .filter_map(|g| c.handles.get(g).cloned())
                .collect()
        })
    }

    /// Waits until the machine is quiet (true) or `deadline` passes (false), stopping whatever registers meanwhile.
    async fn wait_quiet(&self, cause: Cause, deadline: tokio::time::Instant) -> bool {
        let mut changes = self.changes();
        loop {
            let fx = self.update(|c| {
                if c.m.quiet() {
                    None
                } else {
                    Some(c.m.stop_all(cause.clone()))
                }
            });
            let Some(fx) = fx else { return true };
            for w in self.run(fx) {
                let _ = tokio::time::timeout_at(deadline, w).await;
            }
            let tick = tokio::time::Instant::now() + Duration::from_millis(100);
            if tokio::time::timeout_at(tick.min(deadline), changes.changed())
                .await
                .is_err()
                && tokio::time::Instant::now() >= deadline
            {
                return false;
            }
        }
    }

    /// Kills what a previous daemon of this stack left running (its recorded pids) and returns those pids. It runs
    /// only while this engine is quiet (nothing starting, running, being stopped or waiting to restart), under the
    /// engine's lock, so no start can begin meanwhile: every record then is a previous daemon's. Otherwise it refuses,
    /// since this engine's own processes match their records (audit 2, minor 8).
    pub fn reconcile(&self) -> Result<Vec<u32>, Error> {
        let run_dir = self.stack().run_dir();
        self.update(|c| {
            if c.m.quiet() {
                Ok(crate::pidfile::sweep(&run_dir))
            } else {
                Err(Error::Failed(
                    "not sweeping leftovers: this engine already runs something; sweep before anything starts".into(),
                ))
            }
        })
    }
}

/// Stops unit `generation` of `name` in a task of its own: its `stop` steps (bounded), then the signals; then tells the
/// machine it is gone.
pub(crate) fn spawn_stop(e: &Engine, name: String, generation: u64) -> JoinHandle<()> {
    let e = e.clone();
    tokio::spawn(async move {
        let handle = e.update(|c| {
            unwatch(c, &name);
            c.handles.get(&generation).cloned()
        });
        let mut killed = Vec::new();
        if let Some(h) = handle {
            tracing::info!("reaping \"{name}\"…");
            // In a hurry (a second stop request, or the shutdown out of time): no stop steps, no patience, a short
            // grace, so a hurried stop ends within what the hurry waits for it.
            killed = if e.update(|c| c.m.hurrying()) {
                h.stop(Duration::ZERO, HURRY_GRACE).await
            } else {
                let patience = stop_steps(&e, &name).await;
                h.stop(patience, STOP_GRACE).await
            };
        }
        let mut fx = e.update(|c| {
            c.handles.remove(&generation);
            c.m.unit_gone(&name, generation, Now::current())
        });
        if !killed.is_empty() {
            // The journal says so: a sleep that ended in a kill is not the clean stop it would otherwise look like.
            let words: Vec<String> = killed.iter().map(crate::exec::Killed::words).collect();
            let how = format!("killed: {}", words.join("; "));
            for f in &mut fx {
                if let crate::machine::Effect::Journal(ev) = f
                    && ev.kind == crate::events::Kind::Sleep
                    && ev.instance == name
                {
                    ev.detail.clone_from(&how);
                }
            }
        }
        // In the shutdown, its going frees what it needed to be stopped next: this stop ends when those have, so the
        // shutdown that waits for the first ones waits for all (and their journal lines are written by then).
        for w in e.run(fx) {
            let _ = w.await;
        }
    })
}

/// A command's `stop` steps, which should make it exit (then it gets [`STOP_PATIENCE`] to do so); the signals are the
/// fallback.
async fn stop_steps(e: &Engine, name: &str) -> Duration {
    let found = e.update(|c| {
        let l = c.live.get(name)?;
        let (_, stop, at) = l.steps.as_ref()?;
        (!stop.is_empty()).then(|| (stop.clone(), at.clone(), l.log.clone()))
    });
    let Some((stop, at, Some(log))) = found else {
        return Duration::ZERO;
    };
    let ran = steps::run(Moment::Stop, &stop, &at, &log, |r| {
        crate::boot::step_event(e, name, r)
    })
    .await;
    match ran {
        Ok(()) => STOP_PATIENCE,
        Err(err) => {
            tracing::warn!("stop for \"{name}\" failed, sending signals: {err}");
            Duration::ZERO
        }
    }
}

/// Stops watching `name`'s files: it no longer runs (a restart watches again when it registers).
fn unwatch(c: &mut crate::engine::Core, name: &str) {
    if let Some(l) = c.live.get_mut(name) {
        l.watcher = None;
    }
}

/// Reports restart `timer` of `name` due after `delay`.
pub(crate) fn spawn_timer(e: &Engine, name: String, timer: u64, delay: Duration) {
    let e = e.clone();
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let fx = e.update(|c| c.m.restart_due(&name, timer));
        e.run(fx);
    });
}

/// Waits for unit `generation` to end; an exit by itself goes to the machine (a stop reports through its own task).
pub(crate) async fn watch(e: Engine, name: String, generation: u64, handle: Handle) {
    let Ended::Exited { success, text } = handle.ended().await else {
        return;
    };
    let fx = e.update(|c| {
        unwatch(c, &name);
        let fx =
            c.m.exited(&name, generation, success, &text, Now::current());
        if !c.m.stopping(&name, generation) {
            c.handles.remove(&generation);
        }
        fx
    });
    e.run(fx);
}
