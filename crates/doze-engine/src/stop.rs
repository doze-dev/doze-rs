//! Stopping, the crash watcher and restart timers: the shell side of the machine's stop, exit and restart decisions.
//! A stop runs as its own task, so a caller that gives up waiting (a dropped future, an aborted reaper) never leaves
//! a process running that nothing tracks: the unit stays the machine's (as stopping) until the task reports it gone.

use crate::engine::Engine;
use crate::error::Error;
use crate::events::Cause;
use crate::exec::STOP_GRACE;
use crate::lifecycle::Now;
use crate::steps::{self, Moment, STOP_PATIENCE};
use crate::supervise::{Ended, Handle};
use std::time::Duration;
use tokio::task::JoinHandle;

/// How long the shutdown waits for starts in flight and stops to finish.
pub(crate) const SHUTDOWN_LIMIT: Duration = Duration::from_secs(120);

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

    /// Stops everything (cancelling starts and restarts) and waits until nothing runs.
    pub async fn stop_all(&self, cause: Cause) {
        let fx = self.update(|c| c.m.stop_all(cause.clone()));
        for w in self.run(fx) {
            let _ = w.await;
        }
        self.wait_quiet(cause).await;
    }

    /// Stops for good: no start and no restart from now on (a start in flight is stopped as soon as it registers),
    /// then everything is stopped. One way.
    pub async fn shutdown(&self) {
        let fx = self.update(|c| c.m.shutdown());
        for w in self.run(fx) {
            let _ = w.await;
        }
        self.wait_quiet(Cause::Shutdown).await;
    }

    /// Waits, within [`SHUTDOWN_LIMIT`], until the machine is quiet, stopping whatever registers meanwhile.
    async fn wait_quiet(&self, cause: Cause) {
        let deadline = tokio::time::Instant::now() + SHUTDOWN_LIMIT;
        let mut changes = self.changes();
        loop {
            let fx = self.update(|c| {
                if c.m.quiet() {
                    None
                } else {
                    Some(c.m.stop_all(cause.clone()))
                }
            });
            let Some(fx) = fx else { return };
            for w in self.run(fx) {
                let _ = w.await;
            }
            let tick = tokio::time::Instant::now() + Duration::from_millis(100);
            if tokio::time::timeout_at(tick.min(deadline), changes.changed())
                .await
                .is_err()
                && tokio::time::Instant::now() >= deadline
            {
                tracing::warn!("stopping took longer than {SHUTDOWN_LIMIT:?}; giving up waiting");
                return;
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
        let handle = e.update(|c| c.handles.get(&generation).cloned());
        if let Some(h) = handle {
            tracing::info!("reaping \"{name}\"…");
            let patience = stop_steps(&e, &name).await;
            h.stop(patience, STOP_GRACE).await;
        }
        let fx = e.update(|c| {
            c.handles.remove(&generation);
            c.m.unit_gone(&name, generation, Now::current())
        });
        e.run(fx);
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
        let fx =
            c.m.exited(&name, generation, success, &text, Now::current());
        if !c.m.stopping(&name, generation) {
            c.handles.remove(&generation);
        }
        fx
    });
    e.run(fx);
}
