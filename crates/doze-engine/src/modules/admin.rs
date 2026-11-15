//! A module instance's data and console (Go's `Admin` and `Console`): its resources, its data actions, a console
//! panel's rows as they come. Each is asked only of a module whose manifest offers it, and only once the instance has
//! started (a module reads its data with the tools its start resolved); nothing here wakes it.

use super::ModuleInstance;
use crate::stack::PlanCtx;
use doze_module_proto::msg::calls::RunAction;
use doze_module_proto::msg::console::{Resource, TailArgs};
use doze_module_proto::msg::hello::caps;
use doze_module_proto::peer::Stream;

impl ModuleInstance {
    /// Whether its module offers data actions and resources (`admin`).
    pub async fn has_admin(&self) -> Result<bool, String> {
        let m = self.pool.get(&self.key).await.map_err(|e| e.message)?;
        Ok(m.manifest.caps & caps::ADMIN != 0)
    }

    /// Its sub-resources (queues, buckets, databases) with a live status line.
    pub async fn resources(&self, ctx: &PlanCtx) -> Result<Vec<Resource>, String> {
        let m = self.pool.get(&self.key).await.map_err(|e| e.message)?;
        let i = self.with_toolchain(ctx).await?.instance;
        m.resources(&i).await.map_err(|e| e.message)
    }

    /// Runs data action `action` on `resource`; what it says it did.
    pub async fn run_action(
        &self,
        ctx: &PlanCtx,
        action: &str,
        resource: &str,
        input: Option<String>,
    ) -> Result<String, String> {
        let m = self.pool.get(&self.key).await.map_err(|e| e.message)?;
        let a = RunAction {
            instance: self.with_toolchain(ctx).await?.instance,
            action: action.into(),
            resource: resource.into(),
            input,
        };
        m.run_action(&a).await.map_err(|e| e.message)
    }

    /// A console panel's rows as they arrive; dropping the stream stops it.
    pub async fn tail(&self, ctx: &PlanCtx, panel: &str, selector: &str) -> Result<Stream, String> {
        let m = self.pool.get(&self.key).await.map_err(|e| e.message)?;
        if m.manifest.caps & caps::CONSOLE == 0 {
            return Err(format!("{} has no console", self.base.engine));
        }
        let a = TailArgs {
            panel: panel.into(),
            selector: selector.into(),
            instance: self.with_toolchain(ctx).await?.instance,
        };
        m.tail(&a).map_err(|e| e.message)
    }

    /// What its declaration says it should have, by kind and name, for telling them from what was made at run time;
    /// `None` when its module cannot say.
    pub async fn declared(
        &self,
        ctx: &PlanCtx,
    ) -> Option<std::collections::BTreeMap<String, std::collections::BTreeSet<String>>> {
        let m = self.pool.get(&self.key).await.ok()?;
        if m.manifest.caps & caps::OBJECTS == 0 {
            return None;
        }
        let objs = m.objects(&self.instance(ctx)).await.ok()?;
        let mut out =
            std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
        for o in objs {
            out.entry(o.kind).or_default().insert(o.name);
        }
        Some(out)
    }
}

impl ModuleInstance {
    /// Its module's manifest (its console and data actions), starting the module process if it is not running; the
    /// instance itself is not woken.
    pub async fn manifest(&self) -> Result<doze_module_proto::msg::hello::Manifest, String> {
        let m = self.pool.get(&self.key).await.map_err(|e| e.message)?;
        Ok(m.manifest.clone())
    }
}

impl ModuleInstance {
    /// The toolchain its last start resolved (its engine build's folder), if it has started.
    pub fn toolchain(&self) -> Option<doze_module_proto::msg::instance::Toolchain> {
        self.toolchain
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl ModuleInstance {
    /// Whether its module moves data between versions (`migrate`).
    pub async fn has_migrate(&self) -> Result<bool, String> {
        let m = self.pool.get(&self.key).await.map_err(|e| e.message)?;
        Ok(m.manifest.caps & caps::MIGRATE != 0)
    }

    /// Asks its module to move the instance's data to the version its block declares; the instance must be stopped.
    /// What the module says it did.
    pub async fn migrate(&self, ctx: &PlanCtx) -> Result<String, String> {
        let m = self.pool.get(&self.key).await.map_err(|e| e.message)?;
        let args = doze_module_proto::msg::run::ResolveArgs {
            instance: self.instance(ctx),
            target: self.target.clone(),
            pins: self
                .pins
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone(),
        };
        m.migrate(&args).await.map_err(|e| e.message)
    }
}
