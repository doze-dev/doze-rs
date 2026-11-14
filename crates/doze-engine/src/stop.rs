//! Stopping (Go's `Stop`, `StopAll`, `BeginShutdown`), and what happens when a unit exits by itself (Go's `watch` and
//! `scheduleRestart`): its needs released, then restarted per its policy, given up on, or left failed.

use crate::engine::Engine;
use crate::error::Error;
use crate::events::{Cause, Event, Kind};
use crate::exec::STOP_GRACE;
use crate::lifecycle::{AfterExit, Exit, Now};
use crate::restart::Restart;
use crate::steps::{self, Moment, STOP_PATIENCE};
use crate::supervise::{Ended, Handle};
use std::sync::atomic::Ordering;
use std::time::Duration;

impl Engine {
    /// Stops `name` if it runs, cancels a pending restart, and releases the needs it held. An intentional stop resets
    /// the restart budget, so nothing respawns behind it. A cancelled restart leaves it reaped (Go left it showing
    /// "crashing … restarting in 2s" for a restart that never came).
    pub async fn stop(&self, name: &str, cause: Cause) -> Result<(), Error> {
        self.service(name)?;
        let (unit, held) = self.update(|c| {
            c.table.reset_restart(name);
            let live = c.live.entry(name.into()).or_default();
            live.restart_gen = live.restart_gen.wrapping_add(1);
            let cancelled = std::mem::take(&mut live.restart_pending);
            live.ready_pending = false;
            let taken = (live.unit.take(), std::mem::take(&mut live.held));
            if cancelled {
                c.table.mark_reaped(name);
            }
            taken
        });
        if unit.is_none() && held.is_empty() {
            return Ok(());
        }
        if let Some((_, h)) = &unit {
            tracing::info!("reaping \"{name}\"…");
            self.stop_unit(name, h).await;
        }
        for h in &held {
            self.release(h);
        }
        crate::pidfile::remove(&self.stack().run_dir(), name);
        self.update(|c| c.table.mark_reaped(name));
        if unit.is_some() {
            self.event(Event::new(Kind::Sleep, name).cause(cause));
        }
        Ok(())
    }

    /// A command's `stop` steps first, which should make it exit; the signals are the fallback.
    async fn stop_unit(&self, name: &str, h: &Handle) {
        let mut patience = Duration::ZERO;
        if let Some((stop, at, log)) = self.stop_steps(name) {
            let ran = steps::run(Moment::Stop, &stop, &at, &log, |r| {
                crate::boot::step_event(self, name, r)
            })
            .await;
            match ran {
                Ok(()) => patience = STOP_PATIENCE,
                Err(e) => tracing::warn!("stop for \"{name}\" failed, sending signals: {e}"),
            }
        }
        h.stop(patience, STOP_GRACE).await;
    }

    /// Stops everything, in parallel, until nothing runs and no restart is pending (a process that respawned mid-sweep
    /// is caught by the next pass).
    pub async fn stop_all(&self, cause: Cause) {
        loop {
            let names: Vec<String> = self.update(|c| {
                c.live
                    .iter()
                    .filter(|(_, l)| l.unit.is_some() || l.restart_pending || !l.held.is_empty())
                    .map(|(n, _)| n.clone())
                    .collect()
            });
            if names.is_empty() {
                return;
            }
            let mut set = tokio::task::JoinSet::new();
            for n in names {
                let (e, cause) = (self.clone(), cause.clone());
                set.spawn(async move {
                    if let Err(err) = e.stop(&n, cause).await {
                        tracing::warn!("stopping \"{n}\": {err}");
                    }
                });
            }
            while set.join_next().await.is_some() {}
        }
    }

    /// Stops for good: no start and no restart from now on, then everything is stopped. One way.
    pub async fn shutdown(&self) {
        self.inner.shutting.store(true, Ordering::SeqCst);
        // A start in flight finishes registering before the sweep can see it; wait for those first.
        self.wait_for_boots().await;
        self.stop_all(Cause::Shutdown).await;
    }

    async fn wait_for_boots(&self) {
        let boots: Vec<_> =
            self.update(|c| c.live.values().filter_map(|l| l.boot.clone()).collect());
        for mut rx in boots {
            let _ = rx
                .wait_for(|p| !matches!(p, crate::engine::Phase::Booting))
                .await;
        }
    }

    /// Kills what a previous daemon of this stack left running (its recorded pids). Call before anything starts.
    pub fn reconcile(&self) -> Vec<u32> {
        crate::pidfile::sweep(&self.stack().run_dir())
    }
}

/// What the watcher of one unit needs to know.
#[derive(Debug)]
pub(crate) struct Watch {
    pub(crate) name: String,
    pub(crate) generation: u64,
    pub(crate) restart: Option<Restart>,
    pub(crate) supervised: bool,
}

/// Waits for the unit to end; if it exited by itself while still the instance's current unit, decides what follows.
pub(crate) async fn watch(e: Engine, w: Watch, handle: Handle) {
    let Ended::Exited { success, text } = handle.ended().await else {
        return;
    };
    let shutting_down = e.shutting_down();
    let decided = e.update(|c| {
        let live = c.live.get_mut(&w.name)?;
        if live.unit.as_ref().map(|(g, _)| *g) != Some(w.generation) {
            return None; // stopped or replaced on purpose
        }
        live.unit = None;
        live.ready_pending = false;
        let held = std::mem::take(&mut live.held);
        if live.boot.is_some() && !live.unit_in_flight {
            // A new start is already in flight (a wake saw it dead first): that start owns the state now.
            return Some((held, AfterExit::Stays));
        }
        let exit = Exit {
            success,
            text: &text,
            restart: w.restart,
            supervised: w.supervised,
            shutting_down,
        };
        Some((held, c.table.exited(&w.name, exit, Now::current())))
    });
    let Some((held, after)) = decided else {
        return;
    };
    for h in &held {
        e.release(h);
    }
    crate::pidfile::remove(&e.stack().run_dir(), &w.name);
    if !shutting_down {
        e.event(Event::new(Kind::Crash, &w.name).detail(text.as_str()));
    }
    match after {
        AfterExit::Restart {
            delay,
            attempt,
            max,
        } => {
            tracing::info!(
                "process \"{}\" exited; restarting ({attempt}/{max}) in {delay:?}",
                w.name
            );
            schedule_restart(&e, &w.name, delay);
        }
        AfterExit::GaveUp { max } => {
            e.event(Event::new(Kind::GiveUp, &w.name).detail(format!("after {max} restarts")));
        }
        AfterExit::Stays => {}
    }
}

/// Starts `name` again after `delay`, unless a stop or the shutdown cancels it first.
fn schedule_restart(e: &Engine, name: &str, delay: Duration) {
    let generation = e.update(|c| {
        let live = c.live.entry(name.into()).or_default();
        live.restart_gen = live.restart_gen.wrapping_add(1);
        live.restart_pending = true;
        live.restart_gen
    });
    let (e, name) = (e.clone(), name.to_string());
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let due = e.update(|c| {
            let live = c.live.get_mut(&name)?;
            (live.restart_pending && live.restart_gen == generation)
                .then(|| live.restart_pending = false)
        });
        if due.is_none() || e.shutting_down() {
            return;
        }
        if let Err(err) = e.wake(&name, Cause::Restart).await {
            tracing::warn!("restart of \"{name}\" failed: {err}");
        }
    });
}
