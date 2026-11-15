//! The machine's start inputs after the decision (PLAN.md 2.2): a start's needs held one by one, its processes
//! registered, then ready, failed or cancelled. Only the start in flight (its `boot` id) moves an instance.

use super::{Effect, Machine, Unit};
use crate::events::{Cause, Event, Kind};
use crate::lifecycle::Now;
use std::time::Duration;

impl Machine {
    pub(super) fn current_boot(&self, name: &str, boot: u64) -> bool {
        self.ctl.get(name).is_some_and(|c| c.boot == Some(boot))
    }

    /// Start `boot` of `name` was cancelled (a stop, `off`, the shutdown) and has dropped what it was doing: the needs
    /// it held are released, and it is asleep, not failed.
    pub fn boot_cancelled(&mut self, name: &str, boot: u64, now: Now) -> Vec<Effect> {
        if !self.current_boot(name, boot) {
            return Vec::new();
        }
        let c = self.ctl(name);
        c.boot = None;
        c.cancel_boot = false;
        let held = std::mem::take(&mut c.held);
        // Only a cancelled fresh start leaves it asleep; a cancelled `on_ready` rerun whose process crashed meanwhile
        // keeps the crash's state (`crashing` with its restart pending, or `failed`).
        let idle = c.unit.is_none()
            && c.stopping.is_empty()
            && self.table.state(name) == Some(crate::state::State::Booting);
        for h in &held {
            self.table.release(h, now);
        }
        if !idle {
            return Vec::new();
        }
        self.table.mark_reaped(name);
        vec![Effect::RemovePids { name: name.into() }]
    }

    /// Whether `name` has a start in flight or a restart pending (what a `crashing` state promises).
    pub fn pending(&self, name: &str) -> bool {
        self.ctl
            .get(name)
            .is_some_and(|c| c.boot.is_some() || c.timer.is_some() || c.restart_now)
    }

    /// Start `boot` of `name` woke `need` and holds it now.
    pub fn need_held(&mut self, name: &str, boot: u64, need: &str) {
        self.table.acquire(need);
        if self.current_boot(name, boot) {
            self.ctl(name).held.push(need.into());
        } else {
            self.table.release(need, Now::current());
        }
    }

    /// Start `boot` of `name` failed before its processes ran: the needs it held are released, it is failed. A start
    /// already cancelled (it failed because of the stop) is cancelled, not failed.
    pub fn boot_failed(
        &mut self,
        name: &str,
        boot: u64,
        msg: &str,
        took: Duration,
        now: Now,
    ) -> Vec<Effect> {
        if !self.current_boot(name, boot) {
            return Vec::new();
        }
        if self.ctl(name).cancel_boot {
            return self.boot_cancelled(name, boot, now);
        }
        let c = self.ctl(name);
        c.boot = None;
        let held = std::mem::take(&mut c.held);
        for h in &held {
            self.table.release(h, now);
        }
        self.table.mark_failed(name, msg);
        vec![Effect::Journal(
            Event::new(Kind::Failed, name).took(took).detail(msg),
        )]
    }

    /// Start `boot`'s processes run, as unit `generation` (returned). `false`, with a `StopUnit` effect, when the start
    /// was cancelled meanwhile (a stop, `off`, the shutdown): the unit is stopped at once and the start fails.
    pub fn register(
        &mut self,
        name: &str,
        boot: u64,
        unit: Unit,
        now: Now,
    ) -> (u64, bool, Vec<Effect>) {
        let generation = self.id();
        let again = self.ctl(name).registered;
        let cancelled =
            !self.current_boot(name, boot) || self.ctl(name).cancel_boot || again || self.shutting;
        if cancelled {
            let c = self.ctl(name);
            let own = c.boot == Some(boot) && !again;
            let held = if own {
                std::mem::take(&mut c.held)
            } else {
                Vec::new()
            };
            if own {
                c.boot = None;
            }
            c.stopping.insert(generation, (Cause::None, held));
            if own {
                // A stop, `off` or the shutdown cancelled it: asleep, not failed.
                self.table.mark_reaped(name);
            }
            return (
                generation,
                false,
                vec![Effect::StopUnit {
                    name: name.into(),
                    generation,
                }],
            );
        }
        let pid = unit.pids.first().copied().unwrap_or(0);
        let c = self.ctl(name);
        c.registered = true;
        c.unit = Some((generation, pid, unit.backend.clone()));
        c.restart = unit.restart;
        c.supervised = unit.supervised;
        c.ready_pending = false;
        self.table
            .set_idle_policy(name, unit.always_on, unit.idle_after);
        self.table.mark_running(name, &unit.backend, pid, now);
        (
            generation,
            true,
            vec![Effect::WritePids {
                name: name.into(),
                pids: unit.pids,
            }],
        )
    }

    /// `on_ready` of `boot` failed: the processes keep running, the instance is failed, the next wake reruns it. A start
    /// already cancelled (its step lost the race with the cancel) is cancelled, not failed: a crash's restart stays
    /// pending and its state is kept (audit 4, N2).
    pub fn on_ready_failed(
        &mut self,
        name: &str,
        boot: u64,
        msg: &str,
        took: Duration,
        now: Now,
    ) -> Vec<Effect> {
        if !self.current_boot(name, boot) {
            return Vec::new();
        }
        if self.ctl(name).cancel_boot {
            return self.boot_cancelled(name, boot, now);
        }
        let c = self.ctl(name);
        c.boot = None;
        c.ready_pending = c.unit.is_some();
        self.table.mark_failed(name, msg);
        vec![Effect::Journal(
            Event::new(Kind::Failed, name).took(took).detail(msg),
        )]
    }

    /// Start `boot` is ready. `false` when its unit is gone meanwhile (it exited while `on_ready` ran).
    pub fn boot_done(
        &mut self,
        name: &str,
        boot: u64,
        took: Duration,
        now: Now,
    ) -> (bool, Vec<Effect>) {
        if !self.current_boot(name, boot) {
            return (false, Vec::new());
        }
        if self.ctl(name).cancel_boot {
            return (false, self.boot_cancelled(name, boot, now));
        }
        let c = self.ctl(name);
        c.boot = None;
        let Some((_, pid, backend)) = c.unit.clone() else {
            return (false, Vec::new());
        };
        if std::mem::take(&mut c.ready_pending) {
            self.table.mark_running(name, &backend, pid, now);
        }
        (
            true,
            vec![Effect::Journal(Event::new(Kind::Ready, name).took(took))],
        )
    }
}
