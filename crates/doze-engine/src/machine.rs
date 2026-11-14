//! Every decision the engine makes, with no I/O (PLAN.md 2.2): inputs in, [`Effect`]s out. The async shell
//! ([`crate::Engine`]) owns processes, tasks and timers and only executes effects; this owns what is running, what
//! holds what, which start is in flight, which restart is pending, and whether the stack is shutting down. Tests drive
//! it with arbitrary input sequences and synthetic time (`tests/machine_props.rs`).
//!
//! A start (boot) is a sequence of inputs: [`Machine::wake`] decides to start it, then [`Machine::need_held`] per need,
//! [`Machine::register`] once its processes run, and [`Machine::boot_done`] or a failure. A unit (the processes of one
//! start) has a generation; an exit, a stop and a pidfile act only on their own generation.

use crate::error::Error;
use crate::events::{Cause, Event, Kind};
use crate::lifecycle::{Now, Table};
use crate::restart::Restart;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

/// What the shell must do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Run start `boot` of `name` (`rerun`: only its `on_ready` again).
    StartBoot {
        name: String,
        boot: u64,
        cause: Cause,
        rerun: bool,
    },
    /// Cancel start `boot` of `name` now (drop what it is doing: its processes go with it), then report
    /// [`Machine::boot_cancelled`]. `why` is what its waiters hear.
    CancelBoot {
        name: String,
        boot: u64,
        why: Cancel,
    },
    /// Stop unit `generation` of `name`, then report [`Machine::unit_gone`].
    StopUnit { name: String, generation: u64 },
    /// Record these pids for `name`.
    WritePids { name: String, pids: Vec<u32> },
    /// Forget `name`'s pids.
    RemovePids { name: String },
    /// Report [`Machine::restart_due`] after `delay`.
    Schedule {
        name: String,
        timer: u64,
        delay: Duration,
    },
    /// Wake `name` because its restart is due.
    RestartNow { name: String },
    /// Publish a journal event.
    Journal(Event),
}

/// Why a start in flight is cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cancel {
    /// A stop, `off` or the shutdown.
    Stopped,
    /// Its processes exited while `on_ready` ran (audit 4, N1): the crash, not a stop, is what the waiter hears.
    Exited,
}

/// What a wake comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Running and ready.
    Running,
    /// A start is in flight: wait for it.
    Join(u64),
    /// A new start: `StartBoot` is among the effects.
    Start(u64),
    /// A previous run is still being stopped or a start cancelled: ask again once it is gone (two copies must never
    /// run against one data folder, and the old one's records must not be overwritten).
    Busy,
    Refuse(Error),
}

/// What a start registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    pub pids: Vec<u32>,
    pub backend: String,
    pub restart: Option<Restart>,
    pub supervised: bool,
    pub always_on: bool,
    pub idle_after: Option<Duration>,
}

#[derive(Debug, Default)]
struct Ctl {
    unit: Option<(u64, u32, String)>,
    /// Units being stopped: why, and the needs they hold until gone.
    stopping: BTreeMap<u64, (Cause, Vec<String>)>,
    held: Vec<String>,
    boot: Option<u64>,
    /// The start in flight has registered its unit (a second register is a mistake, and is stopped).
    registered: bool,
    cancel_boot: bool,
    timer: Option<u64>,
    /// A restart timer went off and its wake has not run yet: a stop in between cancels it (audit 3, minor 1).
    restart_now: bool,
    ready_pending: bool,
    restart: Option<Restart>,
    supervised: bool,
}

/// The decisions of one stack. See the module docs.
#[derive(Debug)]
pub struct Machine {
    table: Table,
    ctl: BTreeMap<String, Ctl>,
    declared: BTreeSet<String>,
    /// The instances that are tasks: a wake of one is shown only once its run decides it has to run.
    tasks: BTreeSet<String>,
    config: String,
    idle: Option<Duration>,
    shutting: bool,
    next: u64,
}

impl Machine {
    /// A machine for these instances; `config` is how errors name the file, `idle` the stack's idle time.
    pub fn new<I: IntoIterator<Item = String>>(
        names: I,
        config: &str,
        idle: Option<Duration>,
    ) -> Machine {
        let mut table = Table::new();
        let declared: BTreeSet<String> = names.into_iter().collect();
        for n in &declared {
            table.declare(n);
        }
        let _ = table.take_transitions(Now::current());
        Machine {
            table,
            ctl: BTreeMap::new(),
            declared,
            tasks: BTreeSet::new(),
            config: config.into(),
            idle,
            shutting: false,
            next: 0,
        }
    }

    /// The lifecycle states, for snapshots and transitions.
    pub fn table(&self) -> &Table {
        &self.table
    }

    /// The table, for the transitions the shell publishes.
    pub fn table_mut(&mut self) -> &mut Table {
        &mut self.table
    }

    fn id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    fn ctl(&mut self, name: &str) -> &mut Ctl {
        self.ctl.entry(name.into()).or_default()
    }

    fn declared(&self, name: &str) -> Result<(), Error> {
        if self.declared.contains(name) {
            Ok(())
        } else {
            Err(Error::NotDeclared {
                name: name.into(),
                config: self.config.clone(),
            })
        }
    }

    /// Whether the stack is shutting down.
    pub fn shutting_down(&self) -> bool {
        self.shutting
    }

    /// Nothing runs, nothing is being stopped or started, no restart is pending.
    pub fn quiet(&self) -> bool {
        self.ctl.values().all(|c| {
            c.unit.is_none()
                && c.stopping.is_empty()
                && c.boot.is_none()
                && c.timer.is_none()
                && !c.restart_now
        })
    }

    /// A wake of `name` for `cause`.
    pub fn wake(&mut self, name: &str, cause: Cause) -> (Decision, Vec<Effect>) {
        if let Err(e) = self.declared(name) {
            return (Decision::Refuse(e), Vec::new());
        }
        if self.shutting {
            return (
                Decision::Refuse(Error::ShuttingDown(name.into())),
                Vec::new(),
            );
        }
        let running = self.table.state(name).is_some_and(|s| s.running());
        let off = self.table.is_off(name);
        let c = self.ctl(name);
        if cause == Cause::Restart && !c.restart_now {
            // The restart this wake was for was cancelled (a stop landed after its timer went off).
            return (
                Decision::Refuse(Error::Failed(format!(
                    "the restart of \"{name}\" was cancelled"
                ))),
                Vec::new(),
            );
        }
        if !c.stopping.is_empty() || (c.boot.is_some() && c.cancel_boot) {
            return (Decision::Busy, Vec::new());
        }
        if let Some(b) = c.boot {
            return (Decision::Join(b), Vec::new());
        }
        if running && c.unit.is_some() && !c.ready_pending {
            return (Decision::Running, Vec::new());
        }
        if off {
            return (Decision::Refuse(Error::Off(name.into())), Vec::new());
        }
        let boot = self.id();
        let c = self.ctl(name);
        c.boot = Some(boot);
        c.registered = false;
        c.cancel_boot = false;
        // A start now supersedes a restart still waiting out its backoff (Go's `TestScheduleRestartSupersedes`).
        c.timer = None;
        c.restart_now = false;
        let rerun = c.ready_pending && c.unit.is_some();
        let mut fx = Vec::new();
        // A task's wake is journalled, and it is `booting`, only once its run decides it has to run
        // ([`Machine::task_runs`]).
        if !self.tasks.contains(name) {
            let mut ev = Event::new(Kind::Wake, name).cause(cause.clone());
            if rerun {
                ev = ev.detail("on_ready again");
            } else {
                self.table.mark_booting(name);
            }
            fx.push(Effect::Journal(ev));
        }
        fx.push(Effect::StartBoot {
            name: name.into(),
            boot,
            cause,
            rerun,
        });
        (Decision::Start(boot), fx)
    }

    fn current_boot(&self, name: &str, boot: u64) -> bool {
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

#[path = "machine_stop.rs"]
mod stop;
#[path = "machine_task.rs"]
mod task;
