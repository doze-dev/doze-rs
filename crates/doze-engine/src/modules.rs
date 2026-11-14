//! Instances a module runs, through the module pool ([`doze_module_host::Pool`]): one module process per engine, started
//! on first use. A start is `decode`, `resolve` (with the lock's pins), `provision`, then `plan`, whose processes the
//! engine runs like any other; `sync` reaches the module's `objects`, `converge` and `prune`.
//!
//! The lock decides which build runs: [`Pinned`] serves a module's `fetch.ensure` with the hash `doze.lock` pins for that
//! build and target, whatever hash the module asked for, so a module cannot widen what a project pinned.

use crate::stack::{BoxFuture, Endpoint, PlanCtx, Planned, Planner, Structure};
use doze_module_host::{Downloads, Pool};
use doze_module_proto::msg::error::Error as ModError;
use doze_module_proto::msg::instance::{Address, Instance, Object, Toolchain};
use doze_module_proto::msg::run::{Ensure, Ensured, Pin, Ready, ResolveArgs, WithToolchain};
use doze_module_proto::msg::value::{Node, Value};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// How long a module's processes may take to be ready.
const READY_WITHIN: Duration = Duration::from_secs(90);

/// The download service with the lock's engine pins in front: a pin for the build and target asked for wins over the
/// hash the module expects.
#[derive(Debug, Clone)]
pub struct Pinned<D> {
    pub inner: D,
    pub pins: Vec<Pin>,
}

impl<D> Pinned<D> {
    /// The hash the lock pins for this build of `engine` on `target`, if it pins one.
    pub fn pinned(&self, engine: &str, full: &str, target: &str) -> Option<&str> {
        self.pins
            .iter()
            .filter(|p| p.engine == engine && p.resolved == full)
            .find_map(|p| p.hashes.get(target))
            .map(String::as_str)
    }
}

impl<D: Downloads> Downloads for Pinned<D> {
    fn resolve_major<'a>(
        &'a self,
        engine: &'a str,
        major: &'a str,
    ) -> doze_module_host::BoxFuture<'a, Result<String, ModError>> {
        self.inner.resolve_major(engine, major)
    }

    fn ensure<'a>(
        &'a self,
        req: &'a Ensure,
    ) -> doze_module_host::BoxFuture<'a, Result<Ensured, ModError>> {
        Box::pin(async move {
            let mut req = req.clone();
            if let Some(sha) = self.pinned(&req.engine, &req.full, &req.target) {
                req.expected_sha = Some(sha.to_string());
            }
            self.inner.ensure(&req).await
        })
    }
}

/// Where the pins a module's `resolve` reports go (the daemon writes them to `doze.lock`, as Go's `up` did).
pub type PinSink = Arc<dyn Fn(&[Pin]) + Send + Sync>;

/// One instance its module plans.
pub struct ModuleInstance {
    pool: Arc<Pool>,
    /// What the config says: name, engine, version, the checked spec (its folders are filled in per call).
    base: Instance,
    /// The lock's pins, and those its module reported since (so a later start needs no mirror).
    pins: Mutex<Vec<Pin>>,
    sink: Option<PinSink>,
    target: String,
    /// The module declares structural objects (its manifest's `objects` capability).
    structured: bool,
    /// The module says doze does not sit in front of it: it stays up (its manifest's `supervised`).
    supervised: bool,
    /// The toolchain the last start resolved, for `converge` and `prune`.
    toolchain: Mutex<Option<Toolchain>>,
}

impl std::fmt::Debug for ModuleInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModuleInstance")
            .field("name", &self.base.name)
            .field("engine", &self.base.engine)
            .finish_non_exhaustive()
    }
}

impl ModuleInstance {
    /// `base` as declared; `pins`: the lock's pins for its engine; `target`: this host's target triple.
    pub fn new(
        pool: Arc<Pool>,
        base: Instance,
        pins: Vec<Pin>,
        target: &str,
        structured: bool,
    ) -> Self {
        ModuleInstance {
            pool,
            base,
            pins: Mutex::new(pins),
            sink: None,
            supervised: false,
            target: target.into(),
            structured,
            toolchain: Mutex::new(None),
        }
    }

    /// The module is supervised: always on, never reaped when idle.
    #[must_use]
    pub fn supervised(mut self, on: bool) -> Self {
        self.supervised = on;
        self
    }

    /// Pins the module reports are passed to `sink` (and kept for this instance's later starts).
    #[must_use]
    pub fn with_pin_sink(mut self, sink: PinSink) -> Self {
        self.sink = Some(sink);
        self
    }

    /// Keeps `new` pins (each replacing the one for its engine and spec) and passes them on.
    fn keep_pins(&self, new: &[Pin]) {
        if new.is_empty() {
            return;
        }
        {
            let mut pins = self.pins.lock().unwrap_or_else(PoisonError::into_inner);
            for p in new {
                pins.retain(|q| !(q.engine == p.engine && q.spec == p.spec));
                pins.push(p.clone());
            }
        }
        if let Some(sink) = &self.sink {
            sink(new);
        }
    }

    fn instance(&self, ctx: &PlanCtx) -> Instance {
        let mut i = self.base.clone();
        i.data_dir = ctx.data_dir.to_string_lossy().into_owned();
        i.socket_dir = ctx.run_dir.to_string_lossy().into_owned();
        i.toolchain = self
            .toolchain
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        i
    }

    async fn plan_inner(&self, ctx: &PlanCtx) -> Result<Planned, String> {
        let m = self
            .pool
            .get(&self.base.engine)
            .await
            .map_err(|e| e.message)?;
        let mut i = self.instance(ctx);
        doze_os::fs::private_dir(&ctx.run_dir).map_err(|e| e.to_string())?;
        m.decode(&i).await.map_err(|e| e.message)?;
        let args = ResolveArgs {
            instance: i.clone(),
            target: self.target.clone(),
            pins: self
                .pins
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
        };
        let resolved = m.resolve(&args).await.map_err(|e| e.message)?;
        self.keep_pins(&resolved.pins);
        let toolchain = resolved.toolchain;
        *self
            .toolchain
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(toolchain.clone());
        i.toolchain = Some(toolchain.clone());
        let with = WithToolchain {
            instance: i,
            toolchain,
        };
        m.provision(&with).await.map_err(|e| e.message)?;
        let plan = m.plan(&with).await.map_err(|e| e.message)?;
        // Where it is reached: the backend the host granted its main listener, else what its first process's check
        // waits on (a module whose check is a command or a log line still has its grant).
        let granted = self
            .base
            .listeners
            .get(doze_module_proto::network::MAIN)
            .map(|g| match &g.backend {
                Address::Tcp(a) => a.clone(),
                Address::Unix(p) => format!("unix:{p}"),
            });
        let backend = granted
            .or_else(|| {
                plan.processes.iter().find_map(|p| match &p.ready {
                    Some(Ready::Tcp(a)) => Some(a.clone()),
                    Some(Ready::Socket(p)) => Some(format!("unix:{p}")),
                    _ => None,
                })
            })
            .unwrap_or_default();
        Ok(Planned {
            plan,
            endpoint: Endpoint { backend },
            restart: None,
            supervised: self.supervised,
            ready_within: READY_WITHIN,
        })
    }

    async fn with_toolchain(&self, ctx: &PlanCtx) -> Result<WithToolchain, String> {
        let i = self.instance(ctx);
        let toolchain = i
            .toolchain
            .clone()
            .ok_or_else(|| format!("{} has not started", self.base.name))?;
        Ok(WithToolchain {
            instance: i,
            toolchain,
        })
    }
}

impl Planner for ModuleInstance {
    fn plan<'a>(&'a self, ctx: &'a PlanCtx) -> BoxFuture<'a, Result<Planned, String>> {
        Box::pin(self.plan_inner(ctx))
    }

    fn structure(&self) -> Option<&dyn Structure> {
        self.structured.then_some(self as &dyn Structure)
    }
}

impl Structure for ModuleInstance {
    fn objects<'a>(&'a self, ctx: &'a PlanCtx) -> BoxFuture<'a, Result<Vec<Object>, String>> {
        Box::pin(async move {
            let m = self
                .pool
                .get(&self.base.engine)
                .await
                .map_err(|e| e.message)?;
            m.objects(&self.instance(ctx)).await.map_err(|e| e.message)
        })
    }

    fn converge<'a>(&'a self, ctx: &'a PlanCtx) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let m = self
                .pool
                .get(&self.base.engine)
                .await
                .map_err(|e| e.message)?;
            let caps = m.manifest.caps;
            if caps & doze_module_proto::msg::hello::caps::CONVERGE == 0 {
                return Ok(());
            }
            let with = self.with_toolchain(ctx).await?;
            m.converge(&with).await.map_err(|e| e.message)
        })
    }

    fn prune<'a>(
        &'a self,
        ctx: &'a PlanCtx,
        removed: &'a [Object],
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let m = self
                .pool
                .get(&self.base.engine)
                .await
                .map_err(|e| e.message)?;
            if m.manifest.caps & doze_module_proto::msg::hello::caps::PRUNE == 0 {
                return Ok(());
            }
            let with = self.with_toolchain(ctx).await?;
            let args = doze_module_proto::msg::calls::PruneArgs {
                instance: with.instance,
                toolchain: with.toolchain,
                removed: removed.to_vec(),
            };
            m.prune(&args).await.map_err(|e| e.message)
        })
    }

    /// The version and the declared block, without where in the file each value was written (an edit elsewhere in
    /// doze.hcl changes nothing here), as sha256 hex.
    fn fingerprint(&self) -> String {
        let spec = without_spans(&self.base.spec);
        let bytes =
            doze_module_proto::msg::encode(&(self.base.version.as_str(), spec)).unwrap_or_default();
        hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&bytes))
    }
}

/// `n` with every source position cleared.
fn without_spans(n: &Node) -> Node {
    let value = match &n.value {
        Value::List(items) => Value::List(items.iter().map(without_spans).collect()),
        Value::Map(m) => Value::Map(
            m.iter()
                .map(|(k, v)| (k.clone(), without_spans(v)))
                .collect(),
        ),
        v => v.clone(),
    };
    Node::bare(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use doze_module_proto::msg::value::Span;

    fn at(start: u32, value: Value) -> Node {
        Node {
            span: Span {
                file: 0,
                start,
                end: start + 1,
            },
            value,
        }
    }

    #[test]
    fn where_a_value_was_written_is_not_part_of_the_structure() {
        let block = |shift: u32, db: &str| {
            let inner = at(shift + 2, Value::String(db.into()));
            let list = at(shift + 1, Value::List(vec![inner]));
            at(shift, Value::Map([("databases".to_string(), list)].into()))
        };
        assert_eq!(
            without_spans(&block(10, "shop")),
            without_spans(&block(90, "shop"))
        );
        assert_ne!(
            without_spans(&block(10, "shop")),
            without_spans(&block(10, "orders"))
        );
        assert_eq!(
            without_spans(&block(10, "shop")),
            Node::bare(Value::Map(
                [(
                    "databases".to_string(),
                    Node::bare(Value::List(vec![Node::bare(Value::String("shop".into()))]))
                )]
                .into()
            ))
        );
    }
}
