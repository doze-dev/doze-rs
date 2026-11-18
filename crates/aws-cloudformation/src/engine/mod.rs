//! The provisioning task: one per stack operation, running beside the actor. It makes, updates and deletes resources in dependency
//! order (independent ones at the same time), reports every step to the stack's record as it happens, rolls back on failure as AWS
//! does, and finishes with the outputs.

mod back;
mod forward;
mod known;
mod launch;
mod outputs;
mod run;

pub(crate) use known::Working;
pub(crate) use run::run;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use aws_peers::Peers;
use aws_wire::service::Caller;
use doze_runtime::{Clock, Lazy, Tasks};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::Config;
use crate::core::{Answer, Core, Request};
use crate::model::{Draft, Output, Previous, Progress, Res};
use crate::tmpl::params::Bound;
use crate::web::Responses;

/// What an operation does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Create,
    Update,
    Delete,
    /// `RollbackStack` of a stack whose create failed.
    RollbackCreate,
    /// `RollbackStack` of a stack whose update failed, or `ContinueUpdateRollback`.
    RollbackUpdate,
}

/// What a failed create does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OnFailure {
    Rollback,
    Delete,
    DoNothing,
}

/// How an operation ended, for a parent stack waiting on a nested one.
#[derive(Clone, Debug, Default)]
pub(crate) struct Outcome {
    pub(crate) status: String,
    pub(crate) reason: Option<String>,
    pub(crate) outputs: Vec<Output>,
}

/// One stack operation, with everything it needs owned.
#[derive(Debug)]
pub(crate) struct Job {
    pub(crate) kind: Kind,
    pub(crate) stack_id: String,
    pub(crate) stack_name: String,
    pub(crate) caller: Caller,
    /// The processed template to provision (the stack's current one for a delete).
    pub(crate) template: Value,
    pub(crate) params: Vec<Bound>,
    /// The stack before an update (an update's rollback goes back to it).
    pub(crate) previous: Option<Previous>,
    /// The stack's resources when the job starts.
    pub(crate) current: BTreeMap<String, Res>,
    pub(crate) on_failure: OnFailure,
    /// Resources a delete leaves in place (`RetainResources`), or a rollback skips (`ResourcesToSkip`).
    pub(crate) retain: BTreeSet<String>,
    pub(crate) retain_except_on_create: bool,
    /// Every export of the region: name to value and exporting stack.
    pub(crate) exports: BTreeMap<String, (String, String)>,
    pub(crate) tags: Vec<(String, String)>,
    pub(crate) notification_arns: Vec<String>,
    pub(crate) change_set: Option<String>,
    /// The stack policy, and the one given for this update only.
    pub(crate) policy: Option<String>,
    /// Who waits for the end (a parent stack).
    pub(crate) done: Option<oneshot::Sender<Outcome>>,
    /// Whether the stack's tags changed (every taggable resource is updated).
    pub(crate) tags_changed: bool,
}

/// What a job reports to: the region's actor, and what resources are made with.
#[derive(Debug)]
pub(crate) struct Runner {
    pub(crate) actor: Arc<Lazy<Core>>,
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) tasks: Tasks,
    pub(crate) responses: Arc<Responses>,
    pub(crate) config: Arc<Config>,
    pub(crate) clock: Arc<dyn Clock>,
}

impl Runner {
    /// Asks the actor, waits for what it changed to be on disk, and starts the jobs the answer carries.
    ///
    /// # Errors
    /// The actor could not be reached.
    pub(crate) async fn ask(self: &Arc<Self>, req: Request) -> Result<Answer, String> {
        let mut answer = self.actor.call(req).await.map_err(|e| e.to_string())?;
        if let Some(saved) = answer.durable.take()
            && let Err(e) = saved.await
        {
            answer.reply = Err(aws_wire::ApiError::internal(format!("could not save: {e}")));
        }
        for job in std::mem::take(&mut answer.jobs) {
            self.start(job);
        }
        Ok(answer)
    }

    /// Runs a job beside the actor.
    pub(crate) fn start(self: &Arc<Self>, job: Job) {
        let me = Arc::clone(self);
        let id = job.stack_id.clone();
        crate::progress::set_running(&id, true);
        self.tasks.spawn(async move {
            run(me, job).await;
            crate::progress::set_running(&id, false);
        });
    }

    /// Reports progress to the stack's record.
    pub(crate) async fn report(self: &Arc<Self>, p: Progress) {
        if let Err(e) = self.ask(Request::Progress(p)).await {
            tracing::warn!("cloudformation: progress could not be recorded: {e}");
        }
    }

    /// Reports events (and nothing else).
    pub(crate) async fn events(self: &Arc<Self>, stack_id: &str, events: Vec<Draft>) {
        self.report(Progress {
            stack_id: stack_id.to_owned(),
            events,
            ..Progress::default()
        })
        .await;
    }
}
