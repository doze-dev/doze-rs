//! The engine handle: a stack at run time. Every decision is the [`Machine`]'s, made under one short lock that is never
//! held across an await; this shell runs what the machine says (tasks, processes, timers, files) and reports back.

use crate::error::{BuildError, Error};
use crate::events::{Cause, Event};
use crate::lifecycle::Now;
use crate::logs::RunLog;
use crate::machine::{Decision, Effect, Machine};
use crate::process::Step;
use crate::stack::{Endpoint, Service, Stack, When};
use crate::state::{Holder, Instance};
use crate::steps::StepEnv;
use crate::supervise::Handle;
use doze_module_proto::msg::run::Ready;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

/// A running stack. Cheap to clone; every clone is the same engine.
#[derive(Debug, Clone)]
pub struct Engine {
    pub(crate) inner: Arc<Inner>,
}

#[derive(Debug)]
pub(crate) struct Inner {
    pub(crate) stack: Stack,
    logs: doze_logstore::Store,
    journal: Option<doze_journal::Journal>,
    core: Mutex<Core>,
    changed: watch::Sender<u64>,
    transitions: broadcast::Sender<Instance>,
    events: broadcast::Sender<Event>,
}

/// The machine and the I/O resources it refers to, behind one lock.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) m: Machine,
    /// Running units by generation.
    pub(crate) handles: BTreeMap<u64, Handle>,
    /// Starts in flight by id.
    pub(crate) boots: BTreeMap<u64, watch::Receiver<Phase>>,
    /// What cancels each start in flight.
    pub(crate) cancels: BTreeMap<u64, watch::Sender<bool>>,
    pub(crate) live: BTreeMap<String, Live>,
}

/// What the shell keeps for one instance's latest start.
#[derive(Debug, Default)]
pub(crate) struct Live {
    pub(crate) endpoint: Endpoint,
    pub(crate) log: Option<RunLog>,
    /// `on_ready` and `stop` steps with where they run.
    pub(crate) steps: Option<(Vec<Step>, Vec<Step>, StepEnv)>,
    /// A long-lived process the engine watches (the health prober looks at these).
    pub(crate) supervised: bool,
    /// Its readiness check, which the health prober repeats.
    pub(crate) health: Option<Ready>,
}

/// Where a start stands.
#[derive(Debug, Clone)]
pub(crate) enum Phase {
    Booting,
    Ready(Endpoint),
    Failed(Error),
}

/// Builds an [`Engine`] from a stack made in code (doze-config fills the same [`Stack`] from doze.hcl).
#[derive(Debug)]
pub struct Builder {
    stack: Stack,
}

impl Builder {
    /// The stack's idle time.
    pub fn idle_timeout(mut self, d: Duration) -> Builder {
        self.stack.idle_timeout = Some(d);
        self
    }

    /// The file errors name (`doze.hcl` unless set).
    pub fn config_name(mut self, name: &str) -> Builder {
        self.stack.config_name = name.into();
        self
    }

    /// doze's own variables for every process, step, hook and probe it runs (see [`crate::process::with_extra`]).
    pub fn extra_env(mut self, env: BTreeMap<String, String>) -> Builder {
        self.stack.extra_env = env;
        self
    }

    /// What the log store keeps per instance.
    pub fn log_limits(mut self, limits: doze_logstore::Options) -> Builder {
        self.stack.log_limits = limits;
        self
    }

    /// Adds a service.
    pub fn service(mut self, s: Service) -> Builder {
        self.stack.services.push(s);
        self
    }

    /// Checks the stack and builds the engine. Call within a tokio runtime.
    pub fn build(self) -> Result<Engine, BuildError> {
        Engine::new(self.stack)
    }
}

impl Engine {
    /// A builder for stack `name`, keeping its state under `data_dir` (`<home>/projects/<slug>`).
    pub fn builder(name: &str, data_dir: impl Into<PathBuf>) -> Builder {
        let stack = Stack {
            name: name.into(),
            config_name: "doze.hcl".into(),
            data_dir: data_dir.into(),
            idle_timeout: None,
            services: Vec::new(),
            extra_env: BTreeMap::new(),
            log_limits: doze_logstore::Options::default(),
        };
        Builder { stack }
    }

    /// The engine for a whole stack.
    pub fn new(stack: Stack) -> Result<Engine, BuildError> {
        crate::graph::check(&stack)?;
        let names = stack.services.iter().map(|s| s.name.clone());
        let m = Machine::new(names, &stack.config_name, stack.idle_timeout);
        let core = Core {
            m,
            handles: BTreeMap::new(),
            boots: BTreeMap::new(),
            cancels: BTreeMap::new(),
            live: BTreeMap::new(),
        };
        let logs = doze_logstore::Store::with_options(stack.logs_dir(), stack.log_limits);
        // The record screens read. Without it the engine still runs (Go's daemon: "nothing will be recorded").
        let journal = match doze_journal::Journal::open(&stack.journal_path(), 0) {
            Ok(j) => Some(j),
            Err(err) => {
                tracing::warn!("journal: {err} (nothing will be recorded)");
                None
            }
        };
        let inner = Inner {
            logs,
            journal,
            stack,
            core: Mutex::new(core),
            changed: watch::channel(0).0,
            transitions: broadcast::channel(256).0,
            events: broadcast::channel(1024).0,
        };
        Ok(Engine {
            inner: Arc::new(inner),
        })
    }

    /// The stack it runs.
    pub fn stack(&self) -> &Stack {
        &self.inner.stack
    }

    /// What every instance printed: runs kept on disk, readable while it runs and after.
    pub fn logs(&self) -> &doze_logstore::Store {
        &self.inner.logs
    }

    /// The journal the engine writes (`None` if it could not be opened); D1's `journal` topic subscribes to it.
    pub fn journal(&self) -> Option<&doze_journal::Journal> {
        self.inner.journal.as_ref()
    }

    fn lock(&self) -> MutexGuard<'_, Core> {
        self.inner
            .core
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `f` on the state, then publishes what changed.
    pub(crate) fn update<R>(&self, f: impl FnOnce(&mut Core) -> R) -> R {
        let now = Now::current();
        let (r, moved, version) = {
            let mut core = self.lock();
            let r = f(&mut core);
            let t = core.m.table_mut();
            (r, t.take_transitions(now), t.version())
        };
        for inst in moved {
            let _ = self.inner.transitions.send(inst);
        }
        self.inner.changed.send_if_modified(|v| {
            let moved = *v != version;
            *v = version;
            moved
        });
        r
    }

    /// Runs the machine's effects; the tasks it started that a caller may wait for (stops).
    pub(crate) fn run(&self, fx: Vec<Effect>) -> Vec<JoinHandle<()>> {
        let mut waits = Vec::new();
        for f in fx {
            match f {
                Effect::Journal(e) => self.event(e),
                Effect::WritePids { name, pids } => self.pids(&name).write(&pids),
                Effect::RemovePids { name } => self.pids(&name).remove(),
                Effect::StopUnit { name, generation } => {
                    waits.push(crate::stop::spawn_stop(self, name, generation))
                }
                Effect::Schedule { name, timer, delay } => {
                    crate::stop::spawn_timer(self, name, timer, delay)
                }
                Effect::RestartNow { name } => {
                    let e = self.clone();
                    tokio::spawn(async move {
                        if let Err(err) = e.wake(&name, Cause::Restart).await {
                            tracing::warn!("restart of \"{name}\" failed: {err}");
                        }
                    });
                }
                // Starts are spawned by the wake that decided them (it knows how far to wake).
                Effect::StartBoot { .. } => {}
                Effect::CancelBoot { boot, .. } => waits.push(self.cancel_boot(boot)),
            }
        }
        waits
    }

    /// Where `name`'s processes are recorded.
    pub(crate) fn pids(&self, name: &str) -> crate::pidfile::Pids {
        crate::pidfile::Pids::new(&self.stack().run_dir(), name)
    }

    /// Cancels start `boot`; the task ends once the start has dropped what it was doing.
    fn cancel_boot(&self, boot: u64) -> JoinHandle<()> {
        let rx = {
            let c = self.lock();
            if let Some(tx) = c.cancels.get(&boot) {
                let _ = tx.send(true);
            }
            c.boots.get(&boot).cloned()
        };
        tokio::spawn(async move {
            if let Some(mut rx) = rx {
                let _ = rx.wait_for(|p| !matches!(p, Phase::Booting)).await;
            }
        })
    }

    /// Records what the engine did: into the journal, then to subscribers of [`Engine::events`].
    pub(crate) fn event(&self, e: Event) {
        tracing::debug!(kind = e.kind.as_str(), instance = %e.instance, detail = %e.detail);
        if let Some(j) = &self.inner.journal {
            j.append(e.to_journal());
        }
        let _ = self.inner.events.send(e);
    }

    pub(crate) fn service(&self, name: &str) -> Result<&Service, Error> {
        let config = self.inner.stack.config_name.clone();
        self.inner.stack.service(name).ok_or(Error::NotDeclared {
            name: name.into(),
            config,
        })
    }

    /// Every instance, sorted by name.
    pub fn snapshot(&self) -> Vec<Instance> {
        self.lock().m.table().snapshot(Now::current())
    }

    /// One instance.
    pub fn get(&self, name: &str) -> Option<Instance> {
        self.lock().m.table().get(name, Now::current())
    }

    /// Transitions as they happen (lossy: a slow reader misses some and should take a fresh [`Engine::snapshot`]).
    pub fn transitions(&self) -> broadcast::Receiver<Instance> {
        self.inner.transitions.subscribe()
    }

    /// A version that moves on every change, connection churn included.
    pub fn changes(&self) -> watch::Receiver<u64> {
        self.inner.changed.subscribe()
    }

    /// What the engine does, as journal events.
    pub fn events(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    /// Wakes `name` (and what it needs, first) and returns where it is reached once ready. Concurrent wakes of a cold
    /// instance share one start.
    pub async fn wake(&self, name: &str, cause: Cause) -> Result<Endpoint, Error> {
        self.wake_when(name, cause, When::Healthy).await
    }

    pub(crate) async fn wake_when(
        &self,
        name: &str,
        cause: Cause,
        when: When,
    ) -> Result<Endpoint, Error> {
        let (rx, start) = self.decide_wake(name, cause).await;
        // The journal's `wake` goes out before the start can wake (and journal) its needs.
        if let Some(s) = start {
            crate::boot::spawn(self, name, when, s);
        }
        let Some(mut rx) = rx? else {
            return Ok(self
                .lock()
                .live
                .get(name)
                .map(|l| l.endpoint.clone())
                .unwrap_or_default());
        };
        let phase = rx.wait_for(|p| !matches!(p, Phase::Booting)).await;
        match phase.as_deref() {
            Ok(Phase::Ready(ep)) => Ok(ep.clone()),
            Ok(Phase::Failed(e)) => Err(e.clone()),
            _ => Err(Error::Failed(format!(
                "the start of \"{name}\" was abandoned"
            ))),
        }
    }

    /// The machine's answer to a wake, waiting while a previous run is still being stopped or cancelled; the effects
    /// are run here, and a new start is handed back to be spawned.
    async fn decide_wake(
        &self,
        name: &str,
        cause: Cause,
    ) -> (
        Result<Option<watch::Receiver<Phase>>, Error>,
        Option<crate::boot::Start>,
    ) {
        let mut changes = self.changes();
        loop {
            // Decided and recorded under one lock: a wake that joins this start must find its receiver.
            let (answer, fx) = self.update(|c| {
                let (d, fx) = c.m.wake(name, cause.clone());
                let rerun = fx
                    .iter()
                    .any(|f| matches!(f, Effect::StartBoot { rerun: true, .. }));
                let answer = match d {
                    Decision::Busy => None,
                    Decision::Running => Some((Ok(None), None)),
                    Decision::Refuse(e) => Some((Err(e), None)),
                    Decision::Join(b) => Some(match c.boots.get(&b).cloned() {
                        Some(rx) => (Ok(Some(rx)), None),
                        None => (
                            Err(Error::Failed(format!("the start of \"{name}\" was lost"))),
                            None,
                        ),
                    }),
                    Decision::Start(b) => {
                        let (tx, rx) = watch::channel(Phase::Booting);
                        let (cancel_tx, cancel) = watch::channel(false);
                        c.boots.insert(b, rx.clone());
                        c.cancels.insert(b, cancel_tx);
                        let s = crate::boot::Start {
                            boot: b,
                            rerun,
                            tx,
                            cancel,
                        };
                        Some((Ok(Some(rx)), Some(s)))
                    }
                };
                (answer, fx)
            });
            self.run(fx);
            if let Some(a) = answer {
                return a;
            }
            // Bounded poll as well as the change feed: a stop's end always moves the version, but never wait blind.
            let _ = tokio::time::timeout(Duration::from_millis(50), changes.changed()).await;
        }
    }

    /// Why `name` is awake: the running services that need it, a pin, client connections. Empty when it is not running.
    pub fn holders(&self, name: &str) -> Vec<Holder> {
        self.lock().m.holders(name, Now::current())
    }

    /// One more client connection to `name` (the proxy calls this once a wake returned).
    pub fn acquire(&self, name: &str) {
        self.update(|c| c.m.acquire(name));
    }

    /// One client connection fewer.
    pub fn release(&self, name: &str) {
        self.update(|c| c.m.release(name, Now::current()));
    }

    /// Flips the pin; returns the new value.
    pub fn toggle_pin(&self, name: &str) -> Result<bool, Error> {
        let (on, fx) = self.update(|c| c.m.toggle_pin(name))?;
        self.run(vec![fx]);
        Ok(on)
    }

    /// Takes `name` out of this run (stops it; every start is refused) or puts it back (nothing starts at once).
    pub async fn set_off(&self, name: &str, off: bool) -> Result<(), Error> {
        let fx = self.update(|c| c.m.set_off(name, off))?;
        for w in self.run(fx) {
            let _ = w.await;
        }
        Ok(())
    }
}
