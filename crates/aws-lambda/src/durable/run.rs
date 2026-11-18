//! The runner's side: `Invoke` of a durable function (qualified only; synchronous waits for the execution to end, asynchronous
//! answers at once), each invocation of an execution, the chained invokes, and what an answer of the actor set in motion.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

use aws_wire::ApiError;
use aws_wire::rest::RestReply;
use aws_wire::service::{Call, Caller};
use bytes::Bytes;
use doze_runtime::{Deadlines, Settle};
use serde_json::{Value, json};
use tokio::sync::oneshot;

use super::answer::{error_object, lambda_caller, outcome_of, sync_limit, sync_reply};
use super::life::{Answered, Begun, DurableAsk, DurableReply, Finish, Start};
use super::{Chained, Execution, Wake, closed, error_text};
use crate::Inner;
use crate::core::{Answer, Ask, Core};
use crate::errors::invalid;
use crate::invoke::Input;
use crate::req::Req;
use crate::resolve::{Resolved, Resolving};

/// Who is waiting for an execution to end (synchronous invokes, chained invokes of a durable function).
pub(crate) type Waiters = Mutex<HashMap<String, Vec<oneshot::Sender<()>>>>;

/// The region timers: a tick of the actor, then what it set in motion.
pub(crate) fn timers(ctx: Arc<doze_runtime::Ctx>, inner: Weak<Inner>) -> Arc<Deadlines<Core>> {
    let settle: Settle<Core> = Box::new(move |answer: Answer| {
        let inner = inner.upgrade();
        Box::pin(async move {
            let Answer::Durable(done) = answer else {
                return None;
            };
            let Answered {
                wake,
                durable,
                region,
                ..
            } = *done;
            if let Some(saved) = durable {
                // WHY: a failed save is the writer's to report; the next tick finds the same state.
                drop(saved.await);
            }
            let next = wake.due.flatten();
            if let Some(inner) = inner {
                inner.after(&region, Wake { due: None, ..wake });
            }
            next
        })
    });
    Deadlines::new(ctx, || Ask::Durable(Box::new(DurableAsk::Tick)), settle)
}

impl Inner {
    pub(crate) async fn durable(
        &self,
        region: &str,
        ask: DurableAsk,
    ) -> Result<Answered, ApiError> {
        match self.ask(region, Ask::Durable(Box::new(ask))).await? {
            Answer::Durable(done) => {
                let mut done = *done;
                if let Some(saved) = done.durable.take() {
                    saved
                        .await
                        .map_err(|e| ApiError::internal(format!("could not save: {e}")))?;
                }
                Ok(done)
            }
            _ => Err(ApiError::internal(
                "Lambda answered a durable execution question with something else",
            )),
        }
    }

    /// Starts what the durable executions on disk were doing when the process stopped, and their timers. Run once at start: nothing
    /// else would wake a region nobody calls after a restart.
    pub(crate) async fn resume_durable(self: &Arc<Self>) {
        for region in self.actors.keys_on_disk() {
            if let Ok(done) = self.durable(&region, DurableAsk::Tick).await {
                self.after(&region, done.wake);
            }
        }
    }

    /// Arms the region's timer, tells the waiters of what ended, and starts the invocations and chained invokes asked for.
    pub(crate) fn after(self: &Arc<Self>, region: &str, wake: Wake) {
        if let Some(due) = wake.due
            && let Ok(actor) = self.actors.actor(region)
        {
            self.timers.arm(region, &actor, due);
        }
        for arn in &wake.closed {
            let waiting = self
                .waiters
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(arn);
            for tx in waiting.into_iter().flatten() {
                if tx.send(()).is_err() {
                    // WHY: the caller stopped waiting (its invoke timed out); the execution ended all the same.
                }
            }
        }
        for arn in wake.runs {
            let (me, region) = (Arc::clone(self), region.to_owned());
            self.tasks
                .spawn(async move { me.invocation(&region, &arn).await });
        }
        for c in wake.chained {
            let (me, region) = (Arc::clone(self), region.to_owned());
            self.tasks
                .spawn(async move { me.chained(&region, c).await });
        }
    }

    fn waiter(&self, arn: &str) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        self.waiters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(arn.to_owned())
            .or_default()
            .push(tx);
        rx
    }

    async fn peek(&self, region: &str, arn: &str) -> Option<Execution> {
        match self
            .durable(region, DurableAsk::Peek(arn.to_owned()))
            .await
            .ok()?
            .reply
        {
            DurableReply::Execution(e) => e.map(|e| *e),
            _ => None,
        }
    }

    /// Starts an execution; `sync` registers a waiter before the first invocation can begin.
    async fn start_execution(
        self: &Arc<Self>,
        region: &str,
        start: Start,
    ) -> Result<(String, bool, Option<oneshot::Receiver<()>>), ApiError> {
        let sync = start.sync;
        let done = self
            .durable(region, DurableAsk::Start(Box::new(start)))
            .await?;
        let DurableReply::Started(started) = done.reply else {
            return Err(ApiError::internal(
                "Lambda answered a start with something else",
            ));
        };
        let rx = match (&started, sync) {
            (Ok((arn, _)), true) => Some(self.waiter(arn)),
            _ => None,
        };
        self.after(region, done.wake);
        let (arn, fresh) = started?;
        Ok((arn, fresh, rx))
    }

    /// `Invoke` of a function with a `DurableConfig`.
    pub(crate) async fn durable_invoke(
        self: &Arc<Self>,
        call: &Call,
        r: &Req,
        rs: Box<Resolved>,
        payload: Bytes,
    ) -> Result<RestReply, ApiError> {
        if r.function()?.qualifier.is_none() {
            // The developer guide: "You cannot use an unqualified identifier (without a version or alias suffix)." The words are (memory).
            return Err(invalid(format!(
                "Durable functions must be invoked with a qualified identifier: add a version, an alias or $LATEST to {} (for example {}:$LATEST).",
                rs.name, rs.name
            )));
        }
        let kind = r.opt("InvocationType").unwrap_or("RequestResponse");
        if kind == "DryRun" {
            return Ok(RestReply {
                json: json!({"StatusCode": 204}),
                ..RestReply::default()
            });
        }
        let config = rs
            .config
            .get("DurableConfig")
            .cloned()
            .unwrap_or(Value::Null);
        let region = call.caller.region.clone();
        let start = Start {
            function: rs.name.clone(),
            version: rs.version.clone(),
            name: r.opt("DurableExecutionName").map(str::to_owned),
            input: String::from_utf8_lossy(&payload).into_owned(),
            sync: kind != "Event",
            account: call.caller.account_id.clone(),
            endpoint: self.endpoint_for(&call.caller),
            scale: self.config.durable_wait_scale.unwrap_or(1.0),
            config: config.clone(),
        };
        let (arn, fresh, rx) = self.start_execution(&region, start).await?;
        if kind == "Event" {
            return Ok(RestReply {
                json: json!({"StatusCode": 202, "DurableExecutionArn": arn}),
                ..RestReply::default()
            });
        }
        let limit = sync_limit(&config, &rs);
        let ended = self.wait_sync(&region, &arn, fresh, rx, limit).await;
        Ok(RestReply {
            json: sync_reply(&arn, &rs.version, ended.as_ref(), limit),
            ..RestReply::default()
        })
    }

    /// Waits for a synchronous invoke's execution to end, up to `limit` seconds: `None` when it has not.
    async fn wait_sync(
        &self,
        region: &str,
        arn: &str,
        fresh: bool,
        rx: Option<oneshot::Receiver<()>>,
        limit: i64,
    ) -> Option<Execution> {
        if !fresh && let Some(e) = self.peek(region, arn).await.filter(|e| closed(&e.status)) {
            return Some(e);
        }
        let wait = Duration::from_secs(u64::try_from(limit).unwrap_or(1));
        tokio::time::timeout(wait, rx?).await.ok()?.ok()?;
        self.peek(region, arn).await
    }

    /// One invocation of an execution: its payload from the actor, the function run, and what it answered back to the actor.
    async fn invocation(self: &Arc<Self>, region: &str, arn: &str) {
        let Ok(done) = self
            .durable(region, DurableAsk::Begin(arn.to_owned()))
            .await
        else {
            return;
        };
        self.after(region, done.wake);
        let DurableReply::Begun(Some(begun)) = done.reply else {
            return;
        };
        let Begun {
            payload,
            function,
            version,
            account,
            endpoint,
        } = *begun;
        let caller = lambda_caller(region, &account, &endpoint);
        let ran = match self
            .resolve_internal(region, &caller, &function, Some(version))
            .await
        {
            Ok(rs) => {
                let input = Input {
                    payload: Bytes::from(payload),
                    client_context: None,
                    trace_id: String::new(),
                };
                self.run_function(&rs, &caller, input).await
            }
            Err(e) => Err(e),
        };
        let (request_id, outcome) = outcome_of(ran);
        let finish = Finish {
            execution: arn.to_owned(),
            request_id,
            outcome,
        };
        if let Ok(done) = self
            .durable(region, DurableAsk::Finish(Box::new(finish)))
            .await
        {
            self.after(region, done.wake);
        }
    }

    async fn resolve_internal(
        &self,
        region: &str,
        caller: &Caller,
        function: &str,
        qualifier: Option<String>,
    ) -> Result<Box<Resolved>, ApiError> {
        let ask = Resolving {
            caller: caller.clone(),
            function: function.to_owned(),
            qualifier,
            source_arn: None,
            draw: 0.0,
            url: false,
            internal: true,
        };
        match self.ask(region, Ask::Resolve(Box::new(ask))).await? {
            Answer::Resolved(done) => done,
            _ => Err(ApiError::internal(
                "Lambda answered a resolution with something else",
            )),
        }
    }

    /// A chained invoke: the function run (a durable one as an execution of its own, waited for), its result handed back.
    async fn chained(self: &Arc<Self>, region: &str, c: Chained) {
        let caller = lambda_caller(region, &c.account, &c.endpoint);
        let outcome = match self
            .resolve_internal(region, &caller, &c.function, None)
            .await
        {
            Err(e) => Err(error_text(&e.code, &e.message)),
            Ok(rs) if rs.config.contains_key("DurableConfig") => self.child(region, &rs, &c).await,
            Ok(rs) => {
                let input = Input {
                    payload: Bytes::from(c.payload.clone()),
                    client_context: None,
                    trace_id: String::new(),
                };
                match self.run_function(&rs, &caller, input).await {
                    Ok(out) if !out.error => {
                        Ok(Some(String::from_utf8_lossy(&out.payload).into_owned()))
                    }
                    Ok(out) => Err(error_object(&out.payload)),
                    Err(e) => Err(error_text(&e.code, &e.message)),
                }
            }
        };
        let ask = DurableAsk::Chained {
            execution: c.execution,
            op: c.op,
            outcome,
        };
        if let Ok(done) = self.durable(region, ask).await {
            self.after(region, done.wake);
        }
    }

    /// A chained invoke of a durable function: an execution of its own, waited for to its end.
    async fn child(
        self: &Arc<Self>,
        region: &str,
        rs: &Resolved,
        c: &Chained,
    ) -> Result<Option<String>, String> {
        let start = Start {
            function: rs.name.clone(),
            version: rs.version.clone(),
            name: None,
            input: c.payload.clone(),
            sync: true,
            account: c.account.clone(),
            endpoint: c.endpoint.clone(),
            scale: self.config.durable_wait_scale.unwrap_or(1.0),
            config: rs
                .config
                .get("DurableConfig")
                .cloned()
                .unwrap_or(Value::Null),
        };
        let (arn, _, rx) = self
            .start_execution(region, start)
            .await
            .map_err(|e| error_text(&e.code, &e.message))?;
        if let Some(rx) = rx {
            drop(rx.await);
        }
        match self.peek(region, &arn).await {
            Some(e) if e.status == "SUCCEEDED" => Ok(e.result),
            Some(e) => Err(e.error.unwrap_or_else(|| {
                error_text(&e.status, "The chained durable execution did not succeed.")
            })),
            None => Err(error_text(
                "ResourceNotFoundException",
                "The chained durable execution is gone.",
            )),
        }
    }
}
