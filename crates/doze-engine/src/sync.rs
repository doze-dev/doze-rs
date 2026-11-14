//! `doze sync` and `doze destroy` (Go's `runtime/apply.go`): the structural objects each instance's module declares are
//! created and updated (`converge`), the ones applied before and no longer declared are dropped (`prune`), and what was
//! applied is recorded in `.doze/state.json`. One sync at a time per project ([`crate::applied::lock`]).
//!
//! The backends are left running, as Go's daemon left them; a destroy stops what it woke.

use crate::applied::{self, Applied, Change};
use crate::engine::Engine;
use crate::error::Error;
use crate::events::Cause;
use crate::stack::{Kind, PlanCtx, Structure};
use doze_module_proto::msg::instance::Object;
use std::collections::BTreeMap;

/// The file Go's daemon writes in an instance's data folder once its structure converged (with the spec's
/// fingerprint); a destroy removes it.
pub const CONVERGED_MARKER: &str = ".doze-converged";

/// What a sync changed, per instance, in the order it was applied.
pub type Synced = Vec<(String, Vec<(Change, Object)>)>;

impl Engine {
    /// Converges `name` (every instance with structure, when `None`) to what it declares and prunes what it no longer
    /// does. An instance that is off is left alone, its data intact.
    pub async fn sync(&self, name: Option<&str>) -> Result<Synced, Error> {
        let path = self.stack().state_file.clone();
        let _held = self.sync_lock(&path).await?;
        let mut st = Applied::load(&path).map_err(|e| io_err("reading", &path, &e))?;
        let mut out = Vec::new();
        for n in self.targets(name)? {
            let off = self.update(|c| c.m.table().is_off(&n));
            let Some(s) = self.structure_of(&n).filter(|_| !off) else {
                continue;
            };
            self.wake(&n, Cause::Command).await?;
            let ctx = self.ctx_for(&n);
            let fail = |e: String| Error::Failed(format!("syncing \"{n}\": {e}"));
            s.converge(&ctx).await.map_err(fail)?;
            let desired = s.objects(&ctx).await.map_err(fail)?;
            let removed = applied::removed(st.objects(&n), &desired);
            if !removed.is_empty() {
                s.prune(&ctx, &removed).await.map_err(fail)?;
            }
            out.push((n.clone(), applied::diff(st.objects(&n), &desired)));
            st.set(&n, desired);
        }
        save_keeping_tasks(&path, st)?;
        Ok(out)
    }

    /// Drops every object applied for `name` (every instance in the state, when `None`) and forgets them. Each is woken
    /// to run the drops, then stopped. One no longer declared is only forgotten: there is nothing to drop it against.
    pub async fn destroy(&self, name: Option<&str>) -> Result<(), Error> {
        let path = self.stack().state_file.clone();
        let _held = self.sync_lock(&path).await?;
        let mut st = Applied::load(&path).map_err(|e| io_err("reading", &path, &e))?;
        let targets: Vec<String> = match name {
            Some(n) => vec![n.to_string()],
            None => st.instances.keys().cloned().collect(),
        };
        for n in targets {
            let prior = st.objects(&n).to_vec();
            if prior.is_empty() {
                continue;
            }
            if let Some(s) = self.structure_of(&n) {
                self.wake(&n, Cause::Command).await?;
                let ctx = self.ctx_for(&n);
                let reverse: Vec<Object> = prior.into_iter().rev().collect();
                let dropped = s.prune(&ctx, &reverse).await;
                let _ = self.stop(&n, Cause::Command).await;
                dropped.map_err(|e| Error::Failed(format!("destroying \"{n}\": {e}")))?;
            }
            st.set(&n, Vec::new());
            // Go's converged marker goes with the structure, or a boot that reads it would take the dropped objects
            // as converged (`Destroy`, apply.go). Absent is the usual case.
            let _ = std::fs::remove_file(self.stack().cluster_dir(&n).join(CONVERGED_MARKER));
        }
        if name.is_none() {
            st.outputs = BTreeMap::new();
        }
        save_keeping_tasks(&path, st)
    }

    async fn sync_lock(&self, path: &std::path::Path) -> Result<applied::Locked, Error> {
        let p = path.to_path_buf();
        tokio::task::spawn_blocking(move || applied::lock(&p))
            .await
            .map_err(|e| Error::Failed(e.to_string()))?
            .map_err(|e| Error::Failed(e.to_string()))
    }

    fn targets(&self, name: Option<&str>) -> Result<Vec<String>, Error> {
        match name {
            Some(n) => {
                self.service(n)?;
                Ok(vec![n.to_string()])
            }
            None => Ok(self
                .stack()
                .services
                .iter()
                .map(|s| s.name.clone())
                .collect()),
        }
    }

    fn structure_of(&self, name: &str) -> Option<&dyn Structure> {
        match &self.service(name).ok()?.kind {
            Kind::Planned(p) => p.structure(),
            _ => None,
        }
    }

    /// What a module call about `name` is told: its folders and where its needs are now.
    fn ctx_for(&self, name: &str) -> PlanCtx {
        let needs = self
            .service(name)
            .map(|s| s.needs.iter().map(|n| n.name.clone()).collect::<Vec<_>>())
            .unwrap_or_default();
        let needs = self.update(|c| {
            needs
                .into_iter()
                .filter_map(|n| c.live.get(&n).map(|l| (n.clone(), l.endpoint.clone())))
                .collect()
        });
        PlanCtx {
            name: name.into(),
            data_dir: self.stack().cluster_dir(name),
            run_dir: self.stack().run_dir().join(name),
            needs,
        }
    }
}

/// Saves what a sync or destroy decided (objects, outputs) over the file as it is now: the tasks the wakes ran wrote
/// their records meanwhile, and those stay (E2 audit 2, M1; Go's `Apply` lost them).
fn save_keeping_tasks(path: &std::path::Path, st: Applied) -> Result<(), Error> {
    let _records = crate::tasks::run::records_lock();
    let mut now = Applied::load(path).map_err(|e| io_err("reading", path, &e))?;
    now.instances = st.instances;
    now.outputs = st.outputs;
    now.save(path).map_err(|e| io_err("writing", path, &e))
}

fn io_err(what: &str, path: &std::path::Path, e: &std::io::Error) -> Error {
    Error::Failed(format!("{what} {}: {e}", path.display()))
}
