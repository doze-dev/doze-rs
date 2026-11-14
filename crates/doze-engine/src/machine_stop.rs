//! The machine's other inputs: an exit by itself, a stop and its end, a due restart, the reaper's tick, the shutdown,
//! holds, pin and off.

use super::{Ctl, Effect, Machine};
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
        // An `on_ready` rerun of the unit that just exited has nothing left to run on: it is cancelled, so the
        // restart's wake starts afresh instead of joining it (audit 3, M1).
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
        fx
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

    /// Stops everything, cancelling starts and restarts.
    pub fn stop_all(&mut self, cause: Cause) -> Vec<Effect> {
        let names: Vec<String> = self
            .ctl
            .iter()
            .filter(|(_, c)| active(c))
            .map(|(n, _)| n.clone())
            .collect();
        let mut fx = Vec::new();
        for n in names {
            fx.extend(self.stop(&n, cause.clone()).unwrap_or_default());
        }
        fx
    }

    /// From now on nothing starts or restarts; everything is stopped. One way.
    pub fn shutdown(&mut self) -> Vec<Effect> {
        self.shutting = true;
        self.stop_all(Cause::Shutdown)
    }

    /// One more client connection.
    pub fn acquire(&mut self, name: &str) {
        self.table.acquire(name);
    }

    /// One client connection fewer.
    pub fn release(&mut self, name: &str, now: Now) {
        self.table.release(name, now);
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
