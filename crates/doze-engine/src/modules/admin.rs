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
        Ok(self.known_manifest().await?.caps & caps::ADMIN != 0)
    }

    /// Its sub-resources (queues, buckets, databases) with a live status line.
    pub async fn resources(&self, ctx: &PlanCtx) -> Result<Vec<Resource>, String> {
        let m = self.pool.get(&self.key).await.map_err(|e| e.describe())?;
        let i = self.with_toolchain(ctx).await?.instance;
        m.resources(&i).await.map_err(|e| e.describe())
    }

    /// Runs data action `action` on `resource`, with its form's values (`Some` when the caller knows forms, even with none filled in); what it says it did. The error is the
    /// module's own, so a form's caller can tell which field it is about.
    pub async fn run_action(
        &self,
        ctx: &PlanCtx,
        action: &str,
        resource: &str,
        input: Option<String>,
        form: Option<std::collections::BTreeMap<String, String>>,
    ) -> Result<String, doze_module_proto::msg::error::Error> {
        let m = self.pool.get(&self.key).await?;
        let a = RunAction {
            instance: self
                .with_toolchain(ctx)
                .await
                .map_err(|e| {
                    doze_module_proto::msg::error::Error::new(
                        doze_module_proto::msg::error::ErrorKind::Unavailable,
                        e,
                    )
                })?
                .instance,
            action: action.into(),
            resource: resource.into(),
            input,
            form: form.is_some(),
            values: form.unwrap_or_default(),
        };
        m.run_action(&a).await
    }

    /// A console panel's rows as they arrive; dropping the stream stops it.
    pub async fn tail(&self, ctx: &PlanCtx, panel: &str, selector: &str) -> Result<Stream, String> {
        if self.lacks(caps::CONSOLE) {
            return Err(format!("{} has no console", self.base.engine));
        }
        let m = self.pool.get(&self.key).await.map_err(|e| e.describe())?;
        if m.manifest.caps & caps::CONSOLE == 0 {
            return Err(format!("{} has no console", self.base.engine));
        }
        let a = TailArgs {
            panel: panel.into(),
            selector: selector.into(),
            instance: self.with_toolchain(ctx).await?.instance,
        };
        m.tail(&a).map_err(|e| e.describe())
    }

    /// What its declaration says it should have, by kind and name, for telling them from what was made at run time;
    /// `None` when its module cannot say.
    pub async fn declared(
        &self,
        ctx: &PlanCtx,
    ) -> Option<std::collections::BTreeMap<String, std::collections::BTreeSet<String>>> {
        if self.lacks(caps::OBJECTS) {
            return None;
        }
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
    /// Its module's manifest (its console and data actions): as its last hello said it, the module process started
    /// only if it never has; the instance itself is not woken.
    pub async fn manifest(&self) -> Result<doze_module_proto::msg::hello::Manifest, String> {
        Ok((*self.known_manifest().await?).clone())
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
        Ok(self.known_manifest().await?.caps & caps::MIGRATE != 0)
    }

    /// Asks its module to move the instance's data to the version its block declares; the instance must be stopped.
    /// What the module says it did.
    pub async fn migrate(&self, ctx: &PlanCtx) -> Result<String, String> {
        let m = self.pool.get(&self.key).await.map_err(|e| e.describe())?;
        let args = doze_module_proto::msg::run::ResolveArgs {
            instance: self.instance(ctx),
            target: self.target.clone(),
            pins: self
                .pins
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone(),
        };
        m.migrate(&args).await.map_err(|e| e.describe())
    }
}
