//! The engine handle: a stack at run time. Wakes coalesce (every caller of a cold instance shares one start), state
//! lives in the sans-IO [`Table`] under a short lock that is never held across an await, and every change is published.

use crate::error::{BuildError, Error};
use crate::events::{Cause, Event, Kind};
use crate::lifecycle::{Now, Table};
use crate::logs::RunLog;
use crate::process::Step;
use crate::stack::{Endpoint, Service, Stack, When};
use crate::state::{Holder, Instance};
use crate::steps::StepEnv;
use crate::supervise::Handle;
use doze_module_proto::msg::run::Ready;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::sync::{broadcast, watch};

/// A running stack. Cheap to clone; every clone is the same engine.
#[derive(Debug, Clone)]
pub struct Engine {
    pub(crate) inner: Arc<Inner>,
}

#[derive(Debug)]
pub(crate) struct Inner {
    pub(crate) stack: Stack,
    core: Mutex<Core>,
    pub(crate) shutting: AtomicBool,
    changed: watch::Sender<u64>,
    transitions: broadcast::Sender<Instance>,
    events: broadcast::Sender<Event>,
}

/// Everything that changes, behind one lock.
#[derive(Debug, Default)]
pub(crate) struct Core {
    pub(crate) table: Table,
    pub(crate) live: BTreeMap<String, Live>,
    pub(crate) next_gen: u64,
}

/// What the engine holds for one instance besides its lifecycle.
#[derive(Debug, Default)]
pub(crate) struct Live {
    /// The running unit and its generation (a watcher acts only on its own).
    pub(crate) unit: Option<(u64, Handle)>,
    pub(crate) endpoint: Endpoint,
    /// Needs it holds while it runs.
    pub(crate) held: Vec<String>,
    /// The start in flight, shared by every waiter.
    pub(crate) boot: Option<watch::Receiver<Phase>>,
    /// The running unit was registered by the start still in flight (not an earlier one).
    pub(crate) unit_in_flight: bool,
    /// A pending restart and its generation (a stop cancels it by moving the generation).
    pub(crate) restart_gen: u64,
    pub(crate) restart_pending: bool,
    /// Running, but left not ready by a failed `on_ready`: the next wake runs that again and nothing else.
    pub(crate) ready_pending: bool,
    pub(crate) log: Option<RunLog>,
    /// `on_ready` and `stop` steps with where they run.
    pub(crate) steps: Option<(Vec<Step>, Vec<Step>, StepEnv)>,
    /// The check the health prober repeats.
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
        Builder {
            stack: Stack {
                name: name.into(),
                config_name: "doze.hcl".into(),
                data_dir: data_dir.into(),
                idle_timeout: None,
                services: Vec::new(),
            },
        }
    }

    /// The engine for a whole stack.
    pub fn new(stack: Stack) -> Result<Engine, BuildError> {
        crate::graph::check(&stack)?;
        let mut core = Core::default();
        for s in &stack.services {
            core.table.declare(&s.name);
        }
        let _ = core.table.take_transitions(Now::current());
        let inner = Inner {
            stack,
            core: Mutex::new(core),
            shutting: AtomicBool::new(false),
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
            (r, core.table.take_transitions(now), core.table.version())
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

    /// Records what the engine did.
    pub(crate) fn event(&self, e: Event) {
        tracing::debug!(kind = e.kind.as_str(), instance = %e.instance, detail = %e.detail);
        let _ = self.inner.events.send(e);
    }

    pub(crate) fn service(&self, name: &str) -> Result<&Service, Error> {
        self.inner
            .stack
            .service(name)
            .ok_or_else(|| Error::NotDeclared {
                name: name.into(),
                config: self.inner.stack.config_name.clone(),
            })
    }

    pub(crate) fn shutting_down(&self) -> bool {
        self.inner.shutting.load(Ordering::SeqCst)
    }

    /// Every instance, sorted by name.
    pub fn snapshot(&self) -> Vec<Instance> {
        self.lock().table.snapshot(Now::current())
    }

    /// One instance.
    pub fn get(&self, name: &str) -> Option<Instance> {
        self.lock().table.get(name, Now::current())
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
        let mut rx = match self.join_or_start(name, cause, when)? {
            Ok(ep) => return Ok(ep),
            Err(rx) => rx,
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

    /// Running: its endpoint. A start in flight: join it. Else start one.
    fn join_or_start(
        &self,
        name: &str,
        cause: Cause,
        when: When,
    ) -> Result<Result<Endpoint, watch::Receiver<Phase>>, Error> {
        if self.shutting_down() {
            return Err(Error::ShuttingDown(name.into()));
        }
        self.service(name)?;
        let mut core = self.lock();
        let running = core.table.state(name).is_some_and(|s| s.running());
        let live = core.live.entry(name.into()).or_default();
        if running && live.unit.as_ref().is_some_and(|(_, h)| h.alive()) && !live.ready_pending {
            return Ok(Ok(live.endpoint.clone()));
        }
        if let Some(rx) = &live.boot {
            return Ok(Err(rx.clone()));
        }
        if core.table.is_off(name) {
            return Err(Error::Off(name.into()));
        }
        let (tx, rx) = watch::channel(Phase::Booting);
        if let Some(live) = core.live.get_mut(name) {
            live.boot = Some(rx.clone());
        }
        drop(core);
        let engine = self.clone();
        let name = name.to_string();
        tokio::spawn(async move {
            let res = crate::boot::boot(&engine, &name, cause, when).await;
            engine.update(|c| {
                if let Some(l) = c.live.get_mut(&name) {
                    l.boot = None;
                    l.unit_in_flight = false;
                }
            });
            let _ = tx.send(match res {
                Ok(ep) => Phase::Ready(ep),
                Err(e) => Phase::Failed(e),
            });
        });
        Ok(Err(rx))
    }

    /// Why `name` is awake: the running services that need it, a pin, client connections. Empty when it is not running.
    pub fn holders(&self, name: &str) -> Vec<Holder> {
        let core = self.lock();
        let Some(inst) = core
            .table
            .get(name, Now::current())
            .filter(|i| i.state.running())
        else {
            return Vec::new();
        };
        let dependants: Vec<String> = core
            .live
            .iter()
            .filter(|(_, l)| l.held.iter().any(|h| h == name))
            .map(|(n, _)| n.clone())
            .collect();
        let clients = inst
            .conns
            .saturating_sub(u32::try_from(dependants.len()).unwrap_or(u32::MAX));
        let mut out: Vec<Holder> = dependants.into_iter().map(Holder::Dependant).collect();
        if inst.keep_awake {
            out.push(Holder::Pin);
        }
        if clients > 0 {
            out.push(Holder::Connections(clients));
        }
        out
    }

    /// One more client connection to `name` (the proxy calls this once a wake returned).
    pub fn acquire(&self, name: &str) {
        self.update(|c| c.table.acquire(name));
    }

    /// One client connection fewer.
    pub fn release(&self, name: &str) {
        self.update(|c| c.table.release(name, Now::current()));
    }

    /// Flips the pin; returns the new value.
    pub fn toggle_pin(&self, name: &str) -> Result<bool, Error> {
        self.service(name)?;
        let on = self.update(|c| c.table.toggle_pin(name));
        let detail = if on { "on" } else { "off" };
        self.event(
            Event::new(Kind::Pin, name)
                .cause(Cause::Command)
                .detail(detail),
        );
        Ok(on)
    }

    /// Takes `name` out of this run (stops it; every start is refused) or puts it back (nothing starts at once).
    pub async fn set_off(&self, name: &str, off: bool) -> Result<(), Error> {
        self.service(name)?;
        self.update(|c| c.table.set_off(name, off));
        if off {
            self.stop(name, Cause::Command).await?;
        }
        Ok(())
    }
}
