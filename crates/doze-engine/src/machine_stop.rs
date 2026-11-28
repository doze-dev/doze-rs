//! The machine's other inputs: an exit by itself, a stop and its end, a due restart, the reaper's tick, the shutdown,
//! holds, pin and off.

use super::{Cancel, Ctl, Effect, Machine};
use crate::error::Error;
use crate::events::{Cause, Event, Kind};
use crate::lifecycle::{AfterExit, Exit, Now};
use crate::state::Holder;

impl Machine {
    /// Unit `generation` of `name` exited by itself. Acts only if it is the current unit.
    pub fn exited(
        &mut self,
        name: &str,
        generation: u64,
        success: bool,
        text: &str,
        now: Now,
    ) -> Vec<Effect> {
        let shutting_down = self.shutting;
        let c = self.ctl(name);
        if c.unit.as_ref().map(|u| u.0) != Some(generation) {
            return Vec::new();
        }
        c.unit = None;
        c.ready_pending = false;
        // A start in flight (an `on_ready` rerun, or the first start's `on_ready`) has nothing left to run on: it is
        // cancelled, so the restart's wake starts afresh instead of joining it (audit 3, M1), and its waiters hear the
        // crash (audit 4, N1).
        let rerun = c.boot.filter(|_| !c.cancel_boot);
        if rerun.is_some() {
            c.cancel_boot = true;
        }
        let held = std::mem::take(&mut c.held);
        let (restart, supervised) = (c.restart, c.supervised);
        for h in &held {
            self.table.release(h, now);
        }
        let exit = Exit {
            success,
            text,
            restart,
            supervised,
            shutting_down,
        };
        let after = self.table.exited(name, exit, now);
        let mut fx = vec![Effect::RemovePids { name: name.into() }];
        if let Some(boot) = rerun {
            fx.push(Effect::CancelBoot {
                name: name.into(),
                boot,
                why: Cancel::Exited,
            });
        }
        if !shutting_down {
            fx.push(Effect::Journal(Event::new(Kind::Crash, name).detail(text)));
        }
        match after {
            AfterExit::Restart { delay, .. } => {
                let timer = self.id();
                self.ctl(name).timer = Some(timer);
                fx.push(Effect::Schedule {
                    name: name.into(),
                    timer,
                    delay,
                });
            }
            AfterExit::GaveUp { max } => {
                let ev = Event::new(Kind::GiveUp, name).detail(format!("after {max} restarts"));
                fx.push(Effect::Journal(ev));
            }
            AfterExit::Stays => {}
        }
        fx.extend(self.shutdown_next());
        fx
    }

    /// A stop of `name`: a pending restart is cancelled (and the budget reset), a start in flight is cancelled (its
    /// processes are stopped as soon as they register), the running unit is stopped. Its needs stay held until it is
    /// gone ([`Machine::unit_gone`]).
    pub fn stop(&mut self, name: &str, cause: Cause) -> Result<Vec<Effect>, Error> {
        self.declared(name)?;
        self.table.reset_restart(name);
        let c = self.ctl(name);
        let cancelled = c.timer.take().is_some() | std::mem::take(&mut c.restart_now);
        let mut fx = Vec::new();
        if let Some(boot) = c.boot {
            c.cancel_boot = true;
            fx.push(Effect::CancelBoot {
                name: name.into(),
                boot,
                why: Cancel::Stopped,
            });
        }
        c.ready_pending = false;
        let unit = c.unit.take();
        if let Some((generation, ..)) = unit {
            let held = std::mem::take(&mut c.held);
            c.stopping.insert(generation, (cause, held));
            fx.push(Effect::StopUnit {
                name: name.into(),
                generation,
            });
        } else if cancelled {
            self.table.mark_reaped(name);
        }
        Ok(fx)
    }

    /// Unit `generation` of `name` is gone after a stop: its needs are released, its pids forgotten, it is reaped (unless
    /// a new start or unit has taken its place).
    pub fn unit_gone(&mut self, name: &str, generation: u64, now: Now) -> Vec<Effect> {
        let c = self.ctl(name);
        let Some((cause, held)) = c.stopping.remove(&generation) else {
            return Vec::new();
        };
        let replaced = c.unit.is_some() || (c.boot.is_some() && !c.registered);
        for h in &held {
            self.table.release(h, now);
        }
        let mut fx = Vec::new();
        if !replaced {
            self.table.mark_reaped(name);
            fx.push(Effect::RemovePids { name: name.into() });
        }
        if cause != Cause::None {
            fx.push(Effect::Journal(Event::new(Kind::Sleep, name).cause(cause)));
        }
        fx.extend(self.shutdown_next());
        fx
    }

    /// While shutting down, the stops a needs release has made possible: what nothing holds any more goes next.
    pub(crate) fn shutdown_next(&mut self) -> Vec<Effect> {
        if self.shutting {
            self.stop_all(Cause::Shutdown)
        } else {
            Vec::new()
        }
    }

    /// Restart `timer` of `name` is due: wake it, unless a stop or the shutdown cancelled it.
    pub fn restart_due(&mut self, name: &str, timer: u64) -> Vec<Effect> {
        let shutting = self.shutting;
        let c = self.ctl(name);
        if c.timer != Some(timer) || shutting {
            return Vec::new();
        }
        c.timer = None;
        c.restart_now = true;
        vec![Effect::RestartNow { name: name.into() }]
    }

    /// The reaper's tick: stops what has been idle for its time, decided and done under one lock, so a connection that
    /// just arrived is never cut off.
    pub fn reap(&mut self, now: Now) -> Vec<Effect> {
        let due = self.table.reapable(self.idle, now);
        let mut fx = Vec::new();
        for name in due {
            fx.extend(self.stop(&name, Cause::Idle).unwrap_or_default());
        }
        fx
    }

    /// When [`Machine::reap`] next has something to stop, if nothing else changes first.
    pub fn next_reap(&self) -> Option<std::time::Instant> {
        self.table.next_reap(self.idle)
    }

    /// Stops everything, cancelling starts and restarts, dependants before what they need: an instance another one still
    /// holds (running, starting, or being stopped) is left for a later pass, made once that one is gone (the shutdown
    /// makes it itself; [`crate::Engine::stop_all`] calls again until quiet). A consumer that says goodbye to its broker
    /// on SIGTERM must find the broker still there: stopped together, the 2026-11-28 larder `down` left one waiting out
    /// its whole grace, then killed. Needs that form a cycle (nothing free, nothing on its way out) are all stopped.
    pub fn stop_all(&mut self, cause: Cause) -> Vec<Effect> {
        // A start already being cancelled is on its way out: asking again would only cancel it twice.
        let active: Vec<String> = self
            .ctl
            .iter()
            .filter(|(_, c)| active(c) && !cancelling(c))
            .map(|(n, _)| n.clone())
            .collect();
        // Only a running unit waits for its dependants: a pending restart or a start in flight serves no one yet, and is
        // cancelled at once.
        let free: Vec<String> = active
            .iter()
            .filter(|n| {
                !self.needed_by_live(n) || self.ctl.get(n.as_str()).is_none_or(|c| c.unit.is_none())
            })
            .cloned()
            .collect();
        let settling = self
            .ctl
            .values()
            .any(|c| !c.stopping.is_empty() || (c.boot.is_some() && c.cancel_boot));
        let names = if self.hurry || (free.is_empty() && !settling) {
            active
        } else {
            free
        };
        let mut fx = Vec::new();
        for n in names {
            fx.extend(self.stop(&n, cause.clone()).unwrap_or_default());
        }
        fx
    }

    /// The shutdown in a hurry: what still runs is stopped now, needed or not (and stops made from here on use a short
    /// grace, [`Machine::hurrying`]). One way.
    pub fn hurry(&mut self) -> Vec<Effect> {
        self.shutting = true;
        self.hurry = true;
        self.stop_all(Cause::Shutdown)
    }

    /// The generations of the units being stopped.
    pub fn stopping_units(&self) -> Vec<u64> {
        self.ctl
            .values()
            .flat_map(|c| c.stopping.keys().copied())
            .collect()
    }

    /// Whether [`Machine::hurry`] was called.
    pub fn hurrying(&self) -> bool {
        self.hurry
    }

    /// From now on nothing starts or restarts; everything is stopped. One way.
    pub fn shutdown(&mut self) -> Vec<Effect> {
        self.shutting = true;
        self.stop_all(Cause::Shutdown)
    }

    /// One more client connection (none for a name no longer declared: a connection that outlived a remove).
    pub fn acquire(&mut self, name: &str) {
        if self.declared.contains(name) {
            self.table.acquire(name);
        }
    }

    /// One client connection fewer (likewise).
    pub fn release(&mut self, name: &str, now: Now) {
        if self.declared.contains(name) {
            self.table.release(name, now);
        }
    }

    /// One more client connection, and the declaration it counts against (`None`: `name` is not declared).
    pub fn acquire_held(&mut self, name: &str) -> Option<u64> {
        if !self.declared.contains(name) {
            return None;
        }
        self.table.acquire(name);
        Some(self.generation.get(name).copied().unwrap_or(0))
    }

    /// One client connection fewer, if `name` is still the declaration it was acquired against: a connection that
    /// outlived a remove and an add of the same name does not count against the new instance.
    pub fn release_held(&mut self, name: &str, generation: u64, now: Now) {
        if self.declared.contains(name)
            && self.generation.get(name).copied().unwrap_or(0) == generation
        {
            self.table.release(name, now);
        }
    }

    /// Flips the pin; the new value and its journal event.
    pub fn toggle_pin(&mut self, name: &str) -> Result<(bool, Effect), Error> {
        self.declared(name)?;
        let on = self.table.toggle_pin(name);
        let ev = Event::new(Kind::Pin, name)
            .cause(Cause::Command)
            .detail(if on { "on" } else { "off" });
        Ok((on, Effect::Journal(ev)))
    }

    /// `off` takes it out of this run (and stops it); `on` puts it back, starting nothing.
    pub fn set_off(&mut self, name: &str, off: bool) -> Result<Vec<Effect>, Error> {
        self.declared(name)?;
        self.table.set_off(name, off);
        if off {
            self.stop(name, Cause::Command)
        } else {
            Ok(Vec::new())
        }
    }

    /// `name` cannot be reached at all, for `why` (a listener of its could not be opened): failed with that, and every
    /// start of it, or of what needs it, is refused with it.
    pub fn set_unreachable(&mut self, name: &str, why: &str) -> Result<Vec<Effect>, Error> {
        self.declared(name)?;
        self.table.set_unreachable(name, why);
        Ok(vec![Effect::Journal(
            Event::new(Kind::Failed, name).detail(why),
        )])
    }

    /// Why `name` is awake: running services that hold it, a pin, client connections.
    pub fn holders(&self, name: &str, now: Now) -> Vec<Holder> {
        let Some(inst) = self.table.get(name, now).filter(|i| i.state.running()) else {
            return Vec::new();
        };
        let dependants: Vec<String> = self
            .ctl
            .iter()
            .filter(|(_, c)| {
                c.held.iter().any(|h| h == name)
                    || c.stopping
                        .values()
                        .any(|(_, h)| h.iter().any(|x| x == name))
            })
            .map(|(n, _)| n.clone())
            .collect();
        let holds: u32 = self.ctl.values().map(|c| holds_on(c, name)).sum();
        let clients = inst.conns.saturating_sub(holds);
        let mut out: Vec<Holder> = dependants.into_iter().map(Holder::Dependant).collect();
        if inst.keep_awake {
            out.push(Holder::Pin);
        }
        if clients > 0 {
            out.push(Holder::Connections(clients));
        }
        out
    }

    /// Whether unit `generation` of `name` is being stopped (its stop task owns it).
    pub fn stopping(&self, name: &str, generation: u64) -> bool {
        self.ctl
            .get(name)
            .is_some_and(|c| c.stopping.contains_key(&generation))
    }

    /// The holds on `name` that come from other instances (for tests: they must equal what its count says, less its
    /// client connections).
    pub fn held_by_instances(&self, name: &str) -> u32 {
        self.ctl.values().map(|c| holds_on(c, name)).sum()
    }

    /// Whether an instance that runs, is starting, or is still being stopped needs `name` (the shutdown stops nothing
    /// while this holds, unless the needs form a cycle or it is in a hurry).
    pub fn needed_by_live(&self, name: &str) -> bool {
        self.ctl.values().any(|c| {
            (c.unit.is_some() || c.boot.is_some()) && c.held.iter().any(|h| h == name)
                || c.stopping
                    .values()
                    .any(|(_, h)| h.iter().any(|x| x == name))
        })
    }
}

fn holds_on(c: &Ctl, name: &str) -> u32 {
    let n = c.held.iter().filter(|h| *h == name).count()
        + c.stopping
            .values()
            .map(|(_, h)| h.iter().filter(|x| *x == name).count())
            .sum::<usize>();
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn active(c: &Ctl) -> bool {
    c.unit.is_some() || c.boot.is_some() || c.timer.is_some() || c.restart_now
}

/// Only a start that is already being cancelled: nothing left for a stop to do.
fn cancelling(c: &Ctl) -> bool {
    c.boot.is_some() && c.cancel_boot && c.unit.is_none() && c.timer.is_none() && !c.restart_now
}
