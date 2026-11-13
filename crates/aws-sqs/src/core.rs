//! The state a SQS service owns, and the dispatch from an operation to the code that does it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use aws_model_sqs::Operation;
use aws_peers::{Mode, Peers};
use aws_schema::arena::Arena;
use aws_schema::build::Object;
use aws_schema::view::{Shape, View};
use aws_wire::ApiError;
use aws_wire::service::{Call, Output, Reply};
use doze_runtime::{Clock, Ctx};
use doze_store::{Ack, StoreError};

use crate::model::Queue;
use crate::persist::Persist;
use crate::url;

/// All of SQS: its queues, and the clock it reads. Owned by one task, so no operation needs a lock.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) queues: BTreeMap<String, Queue>,
    /// When each deleted queue was deleted: its name cannot be used again for a minute.
    pub(crate) deleted: BTreeMap<String, i64>,
    pub(crate) tasks: Vec<crate::moves::Task>,
    /// Set by a receive that found nothing and may be waited on: how many seconds the caller is willing to wait.
    pub(crate) poll: Option<u32>,
    pub(crate) clock: Arc<dyn Clock>,
    /// The database, when there is one (a test of the rules alone has none).
    pub(crate) persist: Option<Persist>,
    /// Queues whose header or FIFO state may have changed in the call being served.
    pub(crate) touched: BTreeSet<String>,
    /// Queues deleted in the call being served.
    pub(crate) removed: Vec<String>,
    /// The bytes last written for `deleted` and `tasks`.
    pub(crate) written_meta: Vec<u8>,
    /// The bus, for asking whether a caller may do what it asks.
    pub(crate) peers: Option<Arc<Peers>>,
}

/// What the actor answers: the reply, whether the caller may wait for a better one (a receive that found nothing), the commit the
/// reply must wait for, and when the oldest message of any queue passes its retention (the deadline the service's timer sleeps to).
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) reply: Reply,
    pub(crate) poll: Option<u32>,
    pub(crate) durable: Option<Ack>,
    pub(crate) next_due: Option<i64>,
}

/// What the actor is asked: a call, or (from the timer) to remove the messages whose retention has passed.
#[derive(Debug)]
pub(crate) enum Ask {
    Call(Box<Call>),
    Tick,
}

/// The index a `DozePeek` call carries: past the model's operations.
pub(crate) const PEEK_OP: u32 = 1 << 16;

impl doze_runtime::Service for Core {
    type Request = Ask;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        let persist = Persist::open(&ctx.dir)?;
        let mut core = Self::restore(Arc::clone(&ctx.clock), Some(persist));
        core.peers = ctx
            .ext
            .as_ref()
            .and_then(|e| Arc::clone(e).downcast::<Peers>().ok());
        Ok(core)
    }

    async fn handle(&mut self, ask: Ask) -> Answer {
        self.poll = None;
        let call = match ask {
            Ask::Call(call) => *call,
            Ask::Tick => {
                let now = self.now_ms();
                self.expire(now, true);
                return self.answer(empty());
            }
        };
        if let Some(peers) = self.peers.clone()
            && peers.mode() != Mode::Off
            && let Err(refusal) = peers.authorize(&call.caller, self.needs(&call)).await
        {
            return Answer {
                reply: Err(refusal),
                poll: None,
                durable: None,
                next_due: self.next_due(),
            };
        }
        let reply = self.dispatch(&call);
        self.answer(reply)
    }

    async fn close(self) {
        if let Some(persist) = self.persist {
            persist.close();
        }
    }
}

impl Core {
    /// An empty SQS with no disk.
    pub(crate) fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            queues: BTreeMap::new(),
            deleted: BTreeMap::new(),
            tasks: Vec::new(),
            poll: None,
            clock,
            persist: None,
            touched: BTreeSet::new(),
            removed: Vec::new(),
            written_meta: Vec::new(),
            peers: None,
        }
    }

    /// The queue called `name`, to be changed: its header and FIFO state are written after the call if they differ.
    pub(crate) fn queue_named(&mut self, name: &str) -> Option<&mut Queue> {
        if !self.touched.contains(name) {
            self.touched.insert(name.to_owned());
        }
        self.queues.get_mut(name)
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// Writes what the call changed and makes the answer.
    fn answer(&mut self, mut reply: Reply) -> Answer {
        let durable = match self.flush() {
            Ok(ack) => ack,
            Err(e) => {
                reply = Err(ApiError::internal(format!("could not save: {e}")));
                None
            }
        };
        Answer {
            reply,
            poll: self.poll.take(),
            durable,
            next_due: self.next_due(),
        }
    }

    /// Removes the messages whose retention has passed. AWS deletes them in its own time; what a caller sees is computed against
    /// the clock in any case (a count leaves them out at once), so this is about the space they take. A call checks each queue's
    /// oldest message only (`thorough` false); the timer, when that message's time comes, goes through every message.
    pub(crate) fn expire(&mut self, now_ms: i64, thorough: bool) {
        for queue in self.queues.values_mut() {
            queue.expire(now_ms, thorough);
        }
    }

    /// When the timer should next run: when the first message of some queue passes its retention. `None` while every queue is empty,
    /// so an idle SQS holds no timer.
    pub(crate) fn next_due(&self) -> Option<i64> {
        self.queues.values().filter_map(Queue::expires_at).min()
    }

    /// Every operation of the model has an arm, and the match is exhaustive: a model that gains an operation stops the build here.
    pub(crate) fn dispatch(&mut self, call: &Call) -> Reply {
        let now = self.now_ms();
        self.expire(now, false);
        if call.op == PEEK_OP {
            return self.peek(call);
        }
        let Some(op) = Operation::from_index(call.op) else {
            return Err(ApiError::internal("unknown operation index"));
        };
        self.advance_tasks(now);
        match op {
            Operation::CreateQueue => self.create_queue(call),
            Operation::DeleteQueue => self.delete_queue(call),
            Operation::GetQueueUrl => self.get_queue_url(call),
            Operation::GetQueueAttributes => self.get_queue_attributes(call),
            Operation::SetQueueAttributes => self.set_queue_attributes(call),
            Operation::ListQueues => self.list_queues(call),
            Operation::PurgeQueue => self.purge_queue(call),
            Operation::SendMessage => self.send_message(call),
            Operation::StartMessageMoveTask => self.start_message_move_task(call),
            Operation::ListMessageMoveTasks => self.list_message_move_tasks(call),
            Operation::CancelMessageMoveTask => self.cancel_message_move_task(call),
            Operation::AddPermission => self.add_permission(call),
            Operation::RemovePermission => self.remove_permission(call),
            Operation::ListDeadLetterSourceQueues => self.list_dead_letter_source_queues(call),
            Operation::TagQueue => self.tag_queue(call),
            Operation::UntagQueue => self.untag_queue(call),
            Operation::ListQueueTags => self.list_queue_tags(call),
            Operation::ReceiveMessage => self.receive_message(call),
            Operation::DeleteMessage => self.delete_message(call),
            Operation::DeleteMessageBatch => self.delete_message_batch(call),
            Operation::SendMessageBatch => self.send_message_batch(call),
            Operation::ChangeMessageVisibilityBatch => self.change_message_visibility_batch(call),
            Operation::ChangeMessageVisibility => self.change_message_visibility(call),
        }
    }

    /// The queue a URL names, for the caller's account.
    pub(crate) fn queue_mut(
        &mut self,
        call: &Call,
        queue_url: &str,
    ) -> Result<&mut Queue, ApiError> {
        let (account, name) = url::parse(queue_url).ok_or_else(|| invalid_address(call))?;
        if account != call.caller.account_id {
            return Err(invalid_address(call));
        }
        self.queue_named(name).ok_or_else(no_such_queue)
    }
}

pub(crate) fn invalid_address(call: &Call) -> ApiError {
    ApiError::modeled(
        "InvalidAddress",
        format!(
            "The address {}/ is not valid for this endpoint.",
            call.caller.endpoint
        ),
    )
}

pub(crate) fn no_such_queue() -> ApiError {
    ApiError::modeled("QueueDoesNotExist", "The specified queue does not exist.")
}

pub(crate) fn missing(parameter: &str) -> ApiError {
    ApiError::new(
        400,
        "MissingParameter",
        format!("The request must contain the parameter {parameter}."),
    )
}

pub(crate) fn invalid_parameter(message: impl Into<String>) -> ApiError {
    ApiError::new(400, "InvalidParameterValue", message)
}

/// The request as a typed view.
pub(crate) fn request<S: Shape>(call: &Call) -> Result<View<'_, S>, ApiError> {
    View::<S>::root(&call.arena, &call.body, call.root)
        .ok_or_else(|| ApiError::internal("the request was not decoded"))
}

/// An answer, written by `fill`.
pub(crate) fn reply<S>(fill: impl FnOnce(&mut Object<'_, S>)) -> Reply {
    let mut arena = Arena::new();
    let mut out = Object::<S>::root(&mut arena);
    fill(&mut out);
    let root = out.finish();
    Ok(Output { arena, root })
}

/// An answer with nothing in it (the operation has no output).
pub(crate) fn empty() -> Reply {
    Ok(Output {
        arena: Arena::new(),
        root: aws_schema::arena::NIL,
    })
}
