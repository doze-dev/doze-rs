//! Deleting: a stack's resources at its delete, what a failed create made, what an update dropped or replaced, and an update's
//! rollback (the stack converged back to its previous template, then cleaned up). Dependents go first; `DeletionPolicy: Retain`
//! (and `RetainResources`) leave a resource where it is with `DELETE_SKIPPED`.

use std::collections::{BTreeMap, BTreeSet};

use tokio::task::JoinSet;

use super::Working;
use super::run::Op;
use crate::model::{Draft, Progress, Res};
use crate::tmpl::{deps, resources};

type Deleted = (String, Res, Result<(), String>);

impl Op {
    /// The resources each resource of either template waits for.
    fn full_graph(&self) -> BTreeMap<String, BTreeSet<String>> {
        let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut add = |t: &serde_json::Value| {
            let all: BTreeSet<String> = resources(t)
                .map(|r| r.keys().cloned().collect())
                .unwrap_or_default();
            for (k, v) in deps::graph(t, &all, &self.w.conditions) {
                out.entry(k).or_default().extend(v);
            }
        };
        add(&self.w.template);
        if let Some(p) = &self.job.previous
            && let Ok(t) = serde_json::from_str(&p.processed)
        {
            add(&t);
        }
        out
    }

    fn policy_of(&self, name: &str, key: &str) -> Option<String> {
        let from = |t: &serde_json::Value| {
            resources(t)
                .and_then(|r| r.get(name))
                .and_then(|r| r.get(key))
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        };
        from(&self.w.template).or_else(|| {
            self.job
                .previous
                .as_ref()
                .and_then(|p| serde_json::from_str::<serde_json::Value>(&p.processed).ok())
                .and_then(|t| from(&t))
        })
    }

    /// Deletes `names` (dependents first, independent ones at once). `create_rollback` lets `RetainExceptOnCreate` delete what this
    /// operation made; `drop_record` takes them off the stack (an update's cleanup) instead of leaving them `DELETE_COMPLETE`. The
    /// names that failed to delete come back.
    pub(super) async fn remove(
        &mut self,
        names: Vec<String>,
        create_rollback: bool,
    ) -> Vec<String> {
        let drop_record = !matches!(
            self.job.kind,
            super::Kind::Delete | super::Kind::Create | super::Kind::RollbackCreate
        );
        let graph = self.full_graph();
        let mut left: BTreeSet<String> = names
            .into_iter()
            .filter(|n| {
                self.w.resources.get(n).is_some_and(|r| {
                    !r.status.starts_with("DELETE_COMPLETE") && r.status != "DELETE_SKIPPED"
                })
            })
            .collect();
        let mut failures = Vec::new();
        let mut running: JoinSet<Deleted> = JoinSet::new();
        let mut busy: BTreeSet<String> = BTreeSet::new();
        loop {
            let before = left.len();
            let ready: Vec<String> = left
                .iter()
                .filter(|n| {
                    !left
                        .iter()
                        .chain(busy.iter())
                        .any(|o| o != *n && graph.get(o).is_some_and(|w| w.contains(*n)))
                })
                .cloned()
                .collect();
            for name in ready {
                left.remove(&name);
                if self
                    .start_delete(&name, create_rollback, drop_record, &mut running)
                    .await
                {
                    busy.insert(name);
                }
            }
            let Some(joined) = running.join_next().await else {
                if left.len() < before {
                    // Something went without a call: what waited on it may go now.
                    continue;
                }
                // Done, or what is left waits behind something that would not go (it stays as it is).
                break;
            };
            let Ok((name, res, r)) = joined else { continue };
            match r {
                Ok(()) => {
                    busy.remove(&name);
                    self.mark(&name, &res, "DELETE_COMPLETE", None, drop_record)
                        .await;
                }
                Err(why) => {
                    // It stays busy: what it depends on cannot go while it is there.
                    self.mark(&name, &res, "DELETE_FAILED", Some(why), false)
                        .await;
                    failures.push(name);
                }
            }
        }
        failures
    }

    /// Starts deleting one resource: retained ones are skipped and ones never made go at once (`false`); the rest are deleted by a
    /// task in `running` (`true`).
    async fn start_delete(
        &mut self,
        name: &str,
        create_rollback: bool,
        drop_record: bool,
        running: &mut JoinSet<Deleted>,
    ) -> bool {
        let Some(res) = self.w.resources.get(name).cloned() else {
            return false;
        };
        let retained = self.job.retain.contains(name)
            || (self.policy_of(name, "DeletionPolicy").as_deref() == Some("Retain")
                && !(create_rollback
                    && self.job.retain_except_on_create
                    && self.created.contains(name)));
        if retained {
            self.mark(name, &res, "DELETE_SKIPPED", None, drop_record)
                .await;
            return false;
        }
        self.mark(name, &res, "DELETE_IN_PROGRESS", None, false)
            .await;
        if res.physical.is_none() || res.skipped {
            self.mark(name, &res, "DELETE_COMPLETE", None, drop_record)
                .await;
            return false;
        }
        let rc = super::launch::context(&self.runner, &self.job, name, &res.ty);
        let name = name.to_owned();
        running.spawn(async move {
            let r = crate::res::delete(&rc, &res).await;
            (name, res, r)
        });
        true
    }

    /// Records a delete step of a resource (an event and its status), taking it off the stack when `drop_record`.
    async fn mark(
        &mut self,
        name: &str,
        res: &Res,
        status: &str,
        reason: Option<String>,
        drop_record: bool,
    ) {
        let mut now = res.clone();
        now.status = status.to_owned();
        now.reason.clone_from(&reason);
        now.updated_ms = self.runner.clock.now_ms();
        let record = if drop_record && (status == "DELETE_COMPLETE" || status == "DELETE_SKIPPED") {
            self.w.resources.remove(name);
            None
        } else {
            self.w.resources.insert(name.to_owned(), now.clone());
            Some(now)
        };
        self.runner
            .report(Progress {
                stack_id: self.job.stack_id.clone(),
                events: vec![Draft {
                    logical: name.to_owned(),
                    physical: res.physical.clone().unwrap_or_default(),
                    ty: res.ty.clone(),
                    status: status.to_owned(),
                    reason,
                    props: None,
                    detailed: None,
                }],
                resources: vec![(name.to_owned(), record)],
                ..Progress::default()
            })
            .await;
    }

    /// Deletes physical resources this operation left behind (a replaced resource's old one, or a rollback's new one). They are not
    /// the stack's resources any more, so nothing is taken off it.
    async fn remove_leftovers(&mut self) {
        for (name, res) in std::mem::take(&mut self.leftovers) {
            let policy = self.policy_of(&name, "UpdateReplacePolicy");
            let ev = |status: &str, reason: Option<String>| Draft {
                logical: name.clone(),
                physical: res.physical.clone().unwrap_or_default(),
                ty: res.ty.clone(),
                status: status.to_owned(),
                reason,
                props: None,
                detailed: None,
            };
            if policy.as_deref() == Some("Retain") && !self.rolling_back {
                self.runner
                    .events(&self.job.stack_id, vec![ev("DELETE_SKIPPED", None)])
                    .await;
                continue;
            }
            self.runner
                .events(&self.job.stack_id, vec![ev("DELETE_IN_PROGRESS", None)])
                .await;
            let rc = super::launch::context(&self.runner, &self.job, &name, &res.ty);
            let done = match crate::res::delete(&rc, &res).await {
                Ok(()) => ev("DELETE_COMPLETE", None),
                Err(why) => ev("DELETE_FAILED", Some(why)),
            };
            self.runner.events(&self.job.stack_id, vec![done]).await;
        }
    }

    /// After a successful update: delete what the template dropped and the old physical resources of replaced ones.
    pub(super) async fn cleanup_after_update(&mut self) {
        for (name, old) in std::mem::take(&mut self.replaced) {
            self.leftovers.push((name, old));
        }
        self.remove_leftovers().await;
        let dropped: Vec<String> = self
            .w
            .resources
            .keys()
            .filter(|k| !self.w.live.contains(*k))
            .cloned()
            .collect();
        drop(self.remove(dropped, false).await);
    }

    /// An update's rollback: what was replaced is put back, the stack converges on its previous template (updates undone in
    /// place), and what the update made is deleted. `from_failure` is set when the update itself failed (the working view is still
    /// the new template's).
    pub(super) async fn rollback_update(&mut self, from_failure: Option<()>) -> super::Outcome {
        self.rolling_back = true;
        let Some(prev) = self.job.previous.clone() else {
            // (memory)
            return self
                .end(
                    "UPDATE_ROLLBACK_FAILED",
                    Some("There is no previous template to roll back to".into()),
                    None,
                    None,
                )
                .await;
        };
        for (name, old) in std::mem::take(&mut self.replaced) {
            if let Some(new) = self.w.resources.insert(name.clone(), old) {
                self.leftovers.push((name, new));
            }
        }
        if from_failure.is_some() {
            let template = serde_json::from_str(&prev.processed).unwrap_or_default();
            match Working::new(
                template,
                &prev.params,
                self.w.pseudo.clone(),
                self.w.exports.clone(),
                self.w.resources.clone(),
            ) {
                Ok(w) => self.w = w,
                Err(e) => {
                    return self
                        .end("UPDATE_ROLLBACK_FAILED", Some(e), None, None)
                        .await;
                }
            }
        }
        self.failed.clear();
        if !self.forward().await {
            let names: Vec<&str> = self.failed.iter().map(|(n, _)| n.as_str()).collect();
            let why = format!(
                "The following resource(s) failed to update: [{}]. ",
                names.join(", ")
            );
            return self
                .end("UPDATE_ROLLBACK_FAILED", Some(why), None, None)
                .await;
        }
        self.stack_status("UPDATE_ROLLBACK_COMPLETE_CLEANUP_IN_PROGRESS", None)
            .await;
        self.remove_leftovers().await;
        let extra: Vec<String> = self
            .w
            .resources
            .keys()
            .filter(|k| !self.w.live.contains(*k))
            .cloned()
            .collect();
        drop(self.remove(extra, true).await);
        self.end("UPDATE_ROLLBACK_COMPLETE", None, None, Some(true))
            .await
    }
}
