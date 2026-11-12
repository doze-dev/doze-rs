//! An operation from start to end: create, update, delete and the rollbacks, with the stack statuses AWS moves through.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::Value;

use super::{Job, Kind, OnFailure, Outcome, Runner, Working};
use crate::model::{Draft, Finish, Output, Progress, Res};
use crate::tmpl::eval::Pseudo;

/// A running operation.
pub(super) struct Op {
    pub(super) runner: Arc<Runner>,
    pub(super) job: Job,
    pub(super) w: Working,
    /// Resources this operation made.
    pub(super) created: BTreeSet<String>,
    /// Resources this operation replaced, with what they were (their old physical resource is deleted at cleanup, or put back by a
    /// rollback).
    pub(super) replaced: BTreeMap<String, Res>,
    /// Physical resources left to delete at cleanup: logical id and the record as it was.
    pub(super) leftovers: Vec<(String, Res)>,
    /// What failed, and whether it was being created.
    pub(super) failed: Vec<(String, bool)>,
    /// Whether this is a rollback (its events say UPDATE, its failures say rollback).
    pub(super) rolling_back: bool,
}

/// Runs a job to its end.
pub(crate) async fn run(runner: Arc<Runner>, mut job: Job) {
    let done = job.done.take();
    let pseudo = Pseudo {
        account: job.caller.account_id.clone(),
        region: job.caller.region.clone(),
        stack_id: job.stack_id.clone(),
        stack_name: job.stack_name.clone(),
        notification_arns: job.notification_arns.clone(),
    };
    let exports = job
        .exports
        .iter()
        .map(|(k, (v, _))| (k.clone(), v.clone()))
        .collect();
    let w = match Working::new(
        job.template.clone(),
        &job.params,
        pseudo,
        exports,
        job.current.clone(),
    ) {
        Ok(w) => w,
        Err(e) => {
            let status = match job.kind {
                Kind::Create => "CREATE_FAILED",
                Kind::Delete => "DELETE_FAILED",
                _ => "UPDATE_FAILED",
            };
            let mut op = Op::new(runner, job, None);
            let outcome = op.end(status, Some(e), None, None).await;
            if let Some(tx) = done {
                drop(tx.send(outcome));
            }
            return;
        }
    };
    let mut op = Op::new(runner, job, Some(w));
    let outcome = match op.job.kind {
        Kind::Create => op.create().await,
        Kind::Update => op.update().await,
        Kind::Delete => op.delete().await,
        Kind::RollbackCreate => op.rollback_create(String::new()).await,
        Kind::RollbackUpdate => op.rollback_update(None).await,
    };
    if let Some(tx) = done {
        drop(tx.send(outcome));
    }
}

impl Op {
    fn new(runner: Arc<Runner>, job: Job, w: Option<Working>) -> Self {
        let w = w.unwrap_or_else(|| Working {
            template: Value::Null,
            params: BTreeMap::new(),
            pseudo: Pseudo::default(),
            conditions: BTreeMap::new(),
            live: BTreeSet::new(),
            resources: job.current.clone(),
            exports: BTreeMap::new(),
        });
        Self {
            runner,
            job,
            w,
            created: BTreeSet::new(),
            replaced: BTreeMap::new(),
            leftovers: Vec::new(),
            failed: Vec::new(),
            rolling_back: false,
        }
    }

    /// A stack-level event and status.
    pub(super) async fn stack_status(&self, status: &str, reason: Option<&str>) {
        self.runner
            .report(Progress {
                stack_id: self.job.stack_id.clone(),
                events: vec![Draft::stack(
                    &self.job.stack_name,
                    &self.job.stack_id,
                    status,
                    reason,
                )],
                status: Some((status.to_owned(), reason.map(str::to_owned))),
                ..Progress::default()
            })
            .await;
    }

    /// The last status, with what the stack ends with.
    pub(super) async fn end(
        &mut self,
        status: &str,
        reason: Option<String>,
        outputs: Option<Vec<Output>>,
        restore: Option<bool>,
    ) -> Outcome {
        let restore = restore
            .filter(|r| *r)
            .and(self.job.previous.as_ref())
            .map(|p| {
                (
                    p.body.clone(),
                    p.processed.clone(),
                    p.params.clone(),
                    p.description.clone(),
                )
            });
        let imports = (status.ends_with("COMPLETE")
            && !status.contains("ROLLBACK")
            && !status.starts_with("DELETE"))
        .then(|| super::outputs::imports(&self.w));
        let change_set = self.job.change_set.clone().map(|id| {
            let ok = !status.contains("ROLLBACK") && !status.ends_with("FAILED");
            (
                id,
                if ok {
                    "EXECUTE_COMPLETE"
                } else {
                    "EXECUTE_FAILED"
                }
                .to_owned(),
            )
        });
        self.runner
            .report(Progress {
                stack_id: self.job.stack_id.clone(),
                events: vec![Draft::stack(
                    &self.job.stack_name,
                    &self.job.stack_id,
                    status,
                    reason.as_deref(),
                )],
                status: Some((status.to_owned(), reason.clone())),
                finish: Some(Finish {
                    outputs: outputs.clone(),
                    restore,
                    imports,
                    change_set,
                }),
                ..Progress::default()
            })
            .await;
        Outcome {
            status: status.to_owned(),
            reason,
            outputs: outputs.unwrap_or_default(),
        }
    }

    /// The names that failed, as AWS lists them in a stack's status reason.
    fn failed_names(&self, creating: bool) -> String {
        self.failed
            .iter()
            .filter(|(_, c)| *c == creating)
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// `The following resource(s) failed to create: [A, B]. ` (memory), and the same for updates.
    fn failure_reason(&self) -> String {
        let mut out = String::new();
        let created = self.failed_names(true);
        if !created.is_empty() {
            out.push_str(&format!(
                "The following resource(s) failed to create: [{created}]. "
            ));
        }
        let updated = self.failed_names(false);
        if !updated.is_empty() {
            out.push_str(&format!(
                "The following resource(s) failed to update: [{updated}]. "
            ));
        }
        if out.is_empty() {
            out = "The stack operation failed. ".into();
        }
        out
    }

    async fn create(&mut self) -> Outcome {
        if self.forward().await {
            match super::outputs::evaluate(&self.w, &self.job) {
                Ok(outputs) => return self.end("CREATE_COMPLETE", None, Some(outputs), None).await,
                Err(why) => self.failed.push((why, true)),
            }
        }
        let reason = self.failure_reason();
        match self.job.on_failure {
            OnFailure::DoNothing => self.end("CREATE_FAILED", Some(reason), None, None).await,
            OnFailure::Delete => {
                // (memory) A create that fails with OnFailure DELETE goes straight to deleting the stack.
                self.stack_status(
                    "DELETE_IN_PROGRESS",
                    Some(&format!("{reason}Delete requested by user.")),
                )
                .await;
                self.delete().await
            }
            OnFailure::Rollback => {
                self.rollback_create(format!("{reason}Rollback requested by user."))
                    .await
            }
        }
    }

    /// Deletes what a failed create made, then (`OnFailure: DELETE`) the stack.
    pub(super) async fn rollback_create(&mut self, reason: String) -> Outcome {
        if !reason.is_empty() {
            self.stack_status("ROLLBACK_IN_PROGRESS", Some(&reason))
                .await;
        }
        let all: Vec<String> = self.w.resources.keys().cloned().collect();
        let failures = self.remove(all, true).await;
        if !failures.is_empty() {
            let why = format!(
                "The following resource(s) failed to delete: [{}]. ",
                failures.join(", ")
            );
            return self.end("ROLLBACK_FAILED", Some(why), None, None).await;
        }
        self.end("ROLLBACK_COMPLETE", None, None, None).await
    }

    async fn update(&mut self) -> Outcome {
        if self.removals_allowed().await && self.forward().await {
            match super::outputs::evaluate(&self.w, &self.job) {
                Ok(outputs) => {
                    self.stack_status("UPDATE_COMPLETE_CLEANUP_IN_PROGRESS", None)
                        .await;
                    self.cleanup_after_update().await;
                    return self.end("UPDATE_COMPLETE", None, Some(outputs), None).await;
                }
                Err(why) => self.failed.push((why, false)),
            }
        }
        let reason = self.failure_reason();
        if self.job.on_failure == OnFailure::DoNothing {
            return self.end("UPDATE_FAILED", Some(reason), None, None).await;
        }
        self.stack_status("UPDATE_ROLLBACK_IN_PROGRESS", Some(&reason))
            .await;
        self.rollback_update(Some(())).await
    }

    /// Whether the stack policy lets the update remove what the template dropped (`Update:Delete`). A refusal fails the update
    /// before anything is changed.
    async fn removals_allowed(&mut self) -> bool {
        let Some(policy) = self.job.policy.clone() else {
            return true;
        };
        let dropped: Vec<(String, String)> = self
            .w
            .resources
            .iter()
            .filter(|(k, _)| !self.w.live.contains(*k))
            .map(|(k, r)| (k.clone(), r.ty.clone()))
            .collect();
        for (name, ty) in dropped {
            if let Err(why) = crate::stackpolicy::check(&policy, "Update:Delete", &name, &ty) {
                let draft = Draft {
                    logical: name.clone(),
                    physical: self
                        .w
                        .resources
                        .get(&name)
                        .and_then(|r| r.physical.clone())
                        .unwrap_or_default(),
                    ty,
                    status: "UPDATE_FAILED".into(),
                    reason: Some(why),
                    props: None,
                    detailed: None,
                };
                self.runner.events(&self.job.stack_id, vec![draft]).await;
                self.failed.push((name, false));
                return false;
            }
        }
        true
    }

    async fn delete(&mut self) -> Outcome {
        let all: Vec<String> = self.w.resources.keys().cloned().collect();
        let failures = self.remove(all, false).await;
        if failures.is_empty() {
            self.end("DELETE_COMPLETE", None, None, None).await
        } else {
            let why = format!(
                "The following resource(s) failed to delete: [{}]. ",
                failures.join(", ")
            );
            self.end("DELETE_FAILED", Some(why), None, None).await
        }
    }
}
