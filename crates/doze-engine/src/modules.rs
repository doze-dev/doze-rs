//! Instances a module runs, through the module pool ([`doze_module_host::Pool`]): one module process per engine, started
//! on first use. A start is `decode`, `resolve` (with the lock's pins), `provision`, then `plan`, whose processes the
//! engine runs like any other; `sync` reaches the module's `objects`, `converge` and `prune`.
//!
//! The lock decides which build runs: [`Pinned`] serves a module's `fetch.ensure` with the hash `doze.lock` pins for that
//! build and target, whatever hash the module asked for, so a module cannot widen what a project pinned.

use crate::stack::{BoxFuture, Endpoint, PlanCtx, Planned, Planner, Structure};
use doze_module_host::{Downloads, Pool};
use doze_module_proto::msg::error::Error as ModError;
use doze_module_proto::msg::instance::{Instance, Object, Toolchain};
use doze_module_proto::msg::run::{Ensure, Ensured, Pin, Ready, ResolveArgs, WithToolchain};
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

/// One instance its module plans.
pub struct ModuleInstance {
    pool: Arc<Pool>,
    /// What the config says: name, engine, version, the checked spec (its folders are filled in per call).
    base: Instance,
    pins: Vec<Pin>,
    target: String,
    /// The module declares structural objects (its manifest's `objects` capability).
    structured: bool,
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
            pins,
            target: target.into(),
            structured,
            toolchain: Mutex::new(None),
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
            pins: self.pins.clone(),
        };
        let toolchain = m.resolve(&args).await.map_err(|e| e.message)?.toolchain;
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
        // Where it is reached: what its first process's check waits on.
        let backend = plan
            .processes
            .iter()
            .find_map(|p| match &p.ready {
                Some(Ready::Tcp(a)) => Some(a.clone()),
                Some(Ready::Socket(p)) => Some(format!("unix:{p}")),
                _ => None,
            })
            .unwrap_or_default();
        Ok(Planned {
            plan,
            endpoint: Endpoint { backend },
            restart: None,
            supervised: false,
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
}
