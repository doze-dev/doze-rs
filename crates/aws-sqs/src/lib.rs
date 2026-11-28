//! SQS: all 23 operations, as a state machine behind a lazy actor. Standard and FIFO queues, batches, message attributes,
//! long polling, tags, permissions, dead-letter queues and move tasks. State is on disk (`persist`): it survives an idle
//! reap and a restart. Every operation the model has is named in `core`'s dispatch.

mod arrivals;
mod attrs;
mod authz;
mod batch;
mod core;
mod elsewhere;
mod fifo;
mod forced;
mod handles;
mod index;
mod messages;
mod model;
mod moves;
mod msgattr;
mod peek;
mod persist;
mod policy;
mod queues;
mod redrive;
mod send;
mod storage;
mod tags;
mod url;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use aws_model_sqs::meta::META;
use aws_model_sqs::tables::TABLES;
use aws_schema::json::Violation;
use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Reply, Service, Web};
use doze_runtime::{Ctx, Deadlines, Keyed};

use crate::arrivals::Arrivals;
pub use crate::arrivals::Watch;
use crate::core::{Answer, Ask, Core};

/// MD5 of some bytes, as lower-case hex.
pub(crate) fn md5_hex(bytes: &[u8]) -> String {
    use md5::{Digest, Md5};
    aws_core::hex::encode(&Md5::digest(bytes))
}

/// The region whose files are kept in the service's directory itself (state written before regions were kept apart is its).
const HOME_REGION: &str = "us-east-1";

/// The SQS service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Sqs {
    /// One actor per region: a queue, parameter, secret or key belongs to the region that made it.
    actors: Keyed<Core>,
    /// The queues receives wait on, each woken by what could give it a message.
    arrivals: Arc<Arrivals>,
    /// One timer per region, asleep until the oldest message there passes its retention, and none while every queue is empty.
    expiry: Arc<Deadlines<Core>>,
}

impl Sqs {
    /// Registers SQS. `ctx` says where its files would live, when it has any, and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        let expiry = Deadlines::new(
            Arc::clone(&ctx),
            || Ask::Tick,
            Box::new(|mut answer: Answer| {
                Box::pin(async move {
                    if let Some(saved) = answer.durable.take() {
                        // WHY: a failed save is the writer's to report; the next call finds the same state and says so.
                        drop(saved.await);
                    }
                    answer.next_due
                })
            }),
        );
        Self {
            arrivals: Arrivals::new(Arc::clone(&ctx.clock)),
            actors: Keyed::new("sqs", ctx, HOME_REGION),
            expiry,
        }
    }

    /// Registers SQS with the local settings: `skip_waits` (`--sqs-skip-60s-waits`) frees a deleted queue's name at once and lets a
    /// queue be purged again at once, where AWS makes both wait 60 seconds.
    #[must_use]
    pub fn with_settings(ctx: Arc<Ctx>, skip_waits: bool) -> Self {
        let peers = ctx
            .ext
            .as_ref()
            .and_then(|e| Arc::clone(e).downcast::<aws_peers::Peers>().ok());
        Self::new(Arc::new(Ctx {
            dir: ctx.dir.clone(),
            idle: ctx.idle,
            tasks: ctx.tasks.clone(),
            clock: Arc::clone(&ctx.clock),
            ext: Some(Arc::new(core::Ext { peers, skip_waits })),
        }))
    }

    /// Whether the region's retention timer is running (for tests).
    #[cfg(test)]
    pub(crate) fn expiry_armed(&self, region: &str) -> bool {
        self.expiry.armed(region)
    }

    /// Watches the queue `queue_arn` names, for a consumer that is not itself a long poll (a Lambda event source mapping): it reads,
    /// and when there is nothing waits for [`Watch::notified`] (enabled before the read) or [`Watch::until_visible`], whichever
    /// comes first. Woken only by what could give the queue a receivable message: a send, a visibility made shorter, a move or a
    /// redrive into it, a forced release, its deletion. `None` for an ARN that is not a queue's.
    #[must_use]
    pub fn watch(&self, queue_arn: &str) -> Option<Watch> {
        let mut parts = queue_arn.split(':');
        let (region, name) = (parts.nth(3)?, parts.nth(1)?);
        Some(self.arrivals.watch(arrivals::key(region, name)))
    }

    /// Wakes the receives that wait on the queues an answer says something arrived in.
    fn tell(&self, region: &str, answer: &Answer) {
        for name in &answer.arrived {
            self.arrivals.tell(&arrivals::key(region, name));
        }
        if let Some(empty) = &answer.empty {
            self.arrivals
                .note(&arrivals::key(region, &empty.queue), empty.next_visible_ms);
        }
    }

    /// Sends the call to the actor of the region it was signed for, and tells the region's timer when a message next expires.
    async fn call_actor(&self, call: Call) -> Result<Answer, String> {
        let region = call.caller.region.clone();
        let actor = self.actors.actor(&region).map_err(|e| e.to_string())?;
        let answer = actor
            .call(Ask::Call(Box::new(call)))
            .await
            .map_err(|e| e.to_string())?;
        self.expiry.arm(&region, &actor, answer.next_due);
        Ok(answer)
    }

    /// Asks the actor, and holds the answer until what it changed is on disk.
    async fn ask(&self, call: Call) -> Answer {
        let mut answer = self.ask_actor(call).await;
        if let Some(saved) = answer.durable.take()
            && let Err(e) = saved.await
        {
            answer.reply = Err(ApiError::internal(format!("could not save: {e}")));
        }
        answer
    }

    async fn ask_actor(&self, call: Call) -> Answer {
        match self.call_actor(call).await {
            Ok(answer) => answer,
            Err(e) => Answer {
                reply: Err(ApiError::internal(e.to_string())),
                poll: None,
                durable: None,
                next_due: None,
                missed: None,
                empty: None,
                arrived: Vec::new(),
            },
        }
    }

    /// A long poll on the queue `queue` names: asks again when something has arrived there, or when its first hidden message
    /// shows, until one is found or `seconds` have passed. The actor is free the whole time. The watch starts before the first ask
    /// here, so an arrival after the ask that found nothing is not missed.
    async fn wait_for_messages(&self, call: &Call, queue: &str, seconds: u32) -> Reply {
        let region = call.caller.region.clone();
        let watch = self.arrivals.watch(arrivals::key(&region, queue));
        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_secs(u64::from(seconds));
        loop {
            let arrived = watch.notified();
            tokio::pin!(arrived);
            arrived.as_mut().enable();
            let again = self.ask(call.duplicate()).await;
            self.tell(&region, &again);
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if again.poll.is_none() || left.is_zero() {
                return again.reply;
            }
            let wait = watch.until_visible().map_or(left, |d| d.min(left));
            tokio::select! {
                () = arrived => {}
                () = tokio::time::sleep(wait) => {}
            }
        }
    }
}

impl Sqs {
    /// The answer for a failed model check, without needing a service value (tests).
    #[cfg(test)]
    pub(crate) fn invalid_for_tests(violation: &Violation) -> ApiError {
        invalid(violation)
    }
}

impl Service for Sqs {
    fn meta(&self) -> &'static ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static Tables {
        &TABLES
    }

    /// The local look at a queue, `/_doze/sqs/<region>/<queue>/peek` (see `peek`).
    fn web(&self) -> Option<&dyn Web> {
        Some(self)
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(async move {
            let receive = aws_model_sqs::Operation::from_index(call.op)
                == Some(aws_model_sqs::Operation::ReceiveMessage);
            let region = call.caller.region.clone();
            // Only a receive may be asked again (when it waits): any other call is handed over as it is.
            let again = receive.then(|| call.duplicate());
            let mut first = self.ask(call).await;
            self.hint_regions(&region, &mut first).await;
            self.tell(&region, &first);
            match (again, first.poll, first.empty.take()) {
                (Some(call), Some(seconds), Some(empty)) => {
                    self.wait_for_messages(&call, &empty.queue, seconds).await
                }
                _ => first.reply,
            }
        })
    }

    /// SQS words a request that fails its model's checks as `MissingParameter` or `InvalidParameterValue`, not as the Coral
    /// front end's `ValidationException`.
    fn invalid(&self, violation: &Violation) -> ApiError {
        invalid(violation)
    }

    fn storage(&self) -> Option<&dyn aws_wire::storage::Storage> {
        Some(self)
    }

    /// The console's Lab brings a queue's in-flight messages back now (`forced`).
    fn forced(&self) -> Option<&dyn aws_wire::forced::Forced> {
        Some(self)
    }
}

fn invalid(violation: &Violation) -> ApiError {
    let member = |ix: u32| TABLES.member(ix).map_or("", |m| m.name);
    match violation {
        Violation::Missing { member: m, .. } => core::missing(member(*m)),
        Violation::Pattern { member: m, .. } | Violation::Length { member: m, .. }
            if member(*m) == "QueueName" =>
        {
            core::invalid_parameter(
                "Can only include alphanumeric characters, hyphens, or underscores. 1 to 80 in length",
            )
        }
        Violation::Syntax { .. } | Violation::TooDeep => ApiError::new(
            400,
            "MalformedInput",
            "The request body could not be parsed.",
        ),
        other => core::invalid_parameter(format!("The request is not valid: {other:?}")),
    }
}
