//! Step Functions: state machines (Standard and Express), versions and aliases, activities, executions run by `aws-asl`, task
//! integrations through the peer bus, Map Runs, `TestState`, as a state machine behind a lazy actor per region. State is on disk:
//! a Standard execution that was waiting (a Wait, a task, a token, a child) carries on after a restart.
//!
//! **The driver sleeps to the next deadline.** After every call the actor says when the earliest thing is due; one task per region
//! sleeps until then. Nothing ticks.
//!
//! **Task calls happen outside the actor**, in tasks of their own (they take time, and the service called may call back): Lambda,
//! SQS, SNS, DynamoDB, EventBridge, S3 and the `aws-sdk:` integrations of the services that run here, made as the machine's role.
//! An integration whose service does not run here fails the state with an error a `Catch` can take (decision D3); nothing is
//! faked. Activities and child executions need no I/O and are done in the actor.
//!
//! Every one of the model's 37 operations is built.

mod activities;
mod aliases;
mod arn;
mod authz;
mod children;
mod core;
mod deliver;
mod drive;
mod effects;
mod executions;
mod history;
mod integ;
mod logging;
mod machines;
mod mapruns;
mod model;
mod redrive;
mod req;
mod role;
mod shared;
mod tags;
mod teststate;
mod timers;
mod tokens;
mod util;
mod versions;

#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::time::Duration;

use aws_model_sfn::Operation;
use aws_model_sfn::meta::META;
use aws_model_sfn::tables::TABLES;
use aws_schema::meta::ServiceMeta;
use aws_schema::patterns::Patterns;
use aws_schema::tables::Tables;
use aws_wire::ApiError;
use aws_wire::handler::reply_json;
use aws_wire::service::{BoxFuture, Call, Caller, Reply, Service};
use doze_runtime::Ctx;

use crate::core::Request;
use crate::shared::{HOME_REGION, Shared};

/// How long `StartSyncExecution` waits (AWS's limit for an Express execution).
const SYNC_LIMIT: Duration = Duration::from_secs(300);
/// How long `GetActivityTask` holds a poll open (AWS's 60 seconds).
const POLL_LIMIT: Duration = Duration::from_secs(60);

/// The Step Functions service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Sfn {
    shared: Arc<Shared>,
    patterns: Patterns,
}

impl Sfn {
    /// Registers Step Functions. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        Self {
            shared: Shared::new(ctx),
            patterns: Patterns::new(&TABLES),
        }
    }

    /// Carries on the executions of every region that has state on disk: the calls that were out are made again (a task is called
    /// at least once), and the timer is armed for what is due. Run once when the process starts.
    pub async fn resume(&self) {
        let dir = self.shared.ctx.dir.clone();
        let mut regions = Vec::new();
        if dir.join(core::FILE).exists() {
            regions.push(HOME_REGION.to_owned());
        }
        if let Ok(entries) = std::fs::read_dir(dir.join("regions")) {
            for e in entries.flatten() {
                if e.path().join(core::FILE).exists()
                    && let Some(name) = e.file_name().to_str()
                {
                    regions.push(name.to_owned());
                }
            }
        }
        for region in regions {
            let caller = Caller {
                access_key_id: "doze".to_owned(),
                account_id: String::new(),
                region: region.clone(),
                endpoint: String::new(),
                service: None,
                session_token: None,
            };
            // WHY: a region that cannot be opened says so to the first caller.
            drop(
                self.shared
                    .ask(&region, &caller, Request::Resume(caller.clone()))
                    .await,
            );
        }
    }

    async fn ask(&self, call: Call) -> Reply {
        let op = Operation::from_index(call.op);
        match op {
            Some(Operation::TestState) => return self.test_state(&call).await,
            Some(Operation::GetActivityTask) => return self.poll(call).await,
            _ => {}
        }
        let included = (op == Some(Operation::StartSyncExecution))
            .then(|| serde_json::from_str::<serde_json::Value>(&call.body).ok())
            .flatten()
            .and_then(|v| {
                v.get("includedData")
                    .and_then(|i| i.as_str())
                    .map(str::to_owned)
            });
        let (region, caller) = (call.caller.region.clone(), call.caller.clone());
        let answer = self
            .shared
            .ask(&region, &caller, Request::Call(call))
            .await?;
        let Some(arn) = answer.effects.sync.clone() else {
            return answer.reply;
        };
        answer.reply?;
        let waiting = self.shared.wait_for(&arn);
        let done = tokio::time::timeout(SYNC_LIMIT, waiting)
            .await
            .map_err(|_| {
                ApiError::internal("the Express execution did not end within five minutes")
            })?
            .map_err(|_| ApiError::internal("the execution's answer was lost"))?;
        let shaped = executions::sync_shape(&done, included.as_deref());
        reply_json(
            &TABLES,
            &self.patterns,
            Operation::StartSyncExecution.index(),
            &shaped,
        )
    }

    /// `TestState`, beside the actor: it keeps nothing.
    async fn test_state(&self, call: &Call) -> Reply {
        if let Some(peers) = &self.shared.peers
            && peers.mode() != aws_peers::Mode::Off
        {
            peers.authorize(&call.caller, authz::needs(call)).await?;
        }
        let r: serde_json::Value =
            serde_json::from_str(&call.body).unwrap_or(serde_json::Value::Null);
        let value = teststate::run(&self.shared, &call.caller, &r).await?;
        reply_json(&TABLES, &self.patterns, call.op, &value)
    }

    /// `GetActivityTask`: asks for a task, and while there is none waits for one to be announced, for up to 60 seconds.
    async fn poll(&self, call: Call) -> Reply {
        let deadline = tokio::time::Instant::now() + POLL_LIMIT;
        let activity = serde_json::from_str::<serde_json::Value>(&call.body)
            .ok()
            .and_then(|v| {
                v.get("activityArn")
                    .and_then(|a| a.as_str())
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        let (region, caller) = (call.caller.region.clone(), call.caller.clone());
        loop {
            let notify = self.shared.poll_slot(&activity);
            let woken = notify.notified();
            tokio::pin!(woken);
            woken.as_mut().enable();
            let answer = self
                .shared
                .ask(&region, &caller, Request::Call(call.duplicate()))
                .await?;
            if answer.effects.taken
                || answer.reply.is_err()
                || tokio::time::Instant::now() >= deadline
            {
                return answer.reply;
            }
            let stopping = self.shared.ctx.tasks.stopping();
            tokio::select! {
                () = &mut woken => {}
                () = tokio::time::sleep_until(deadline) => return answer.reply,
                () = stopping.cancelled() => return answer.reply,
            }
        }
    }
}

impl Service for Sfn {
    fn meta(&self) -> &'static ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static Tables {
        &TABLES
    }

    fn serves(&self, op: u32) -> bool {
        Operation::from_index(op).is_some_and(core::built)
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(self.ask(call))
    }
}
