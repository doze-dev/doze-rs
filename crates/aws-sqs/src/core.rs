//! The state a SQS service owns, and the dispatch from an operation to the code that does it.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::Arc;

use aws_model_sqs::Operation;
use aws_schema::arena::Arena;
use aws_schema::build::Object;
use aws_schema::view::{Shape, View};
use aws_wire::ApiError;
use aws_wire::service::{Call, Output, Reply};
use doze_runtime::{Clock, Ctx};

use crate::model::Queue;
use crate::url;

/// All of SQS: its queues, and the clock it reads. Owned by one task, so no operation needs a lock.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) queues: BTreeMap<String, Queue>,
    /// When each deleted queue was deleted: its name cannot be used again for a minute.
    pub(crate) deleted: BTreeMap<String, i64>,
    pub(crate) clock: Arc<dyn Clock>,
}

impl doze_runtime::Service for Core {
    type Request = Call;
    type Response = Reply;
    type Error = Infallible;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, Infallible> {
        Ok(Self {
            queues: BTreeMap::new(),
            deleted: BTreeMap::new(),
            clock: Arc::clone(&ctx.clock),
        })
    }

    async fn handle(&mut self, call: Call) -> Reply {
        self.dispatch(&call)
    }
}

impl Core {
    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// Every operation of the model has an arm: the ones this build does not do are refused by name, on purpose.
    pub(crate) fn dispatch(&mut self, call: &Call) -> Reply {
        let Some(op) = Operation::from_index(call.op) else {
            return Err(ApiError::internal("unknown operation index"));
        };
        match op {
            Operation::CreateQueue => self.create_queue(call),
            Operation::DeleteQueue => self.delete_queue(call),
            Operation::GetQueueUrl => self.get_queue_url(call),
            Operation::GetQueueAttributes => self.get_queue_attributes(call),
            Operation::SetQueueAttributes => self.set_queue_attributes(call),
            Operation::ListQueues => self.list_queues(call),
            Operation::PurgeQueue => self.purge_queue(call),
            Operation::SendMessage => self.send_message(call),
            Operation::TagQueue => self.tag_queue(call),
            Operation::UntagQueue => self.untag_queue(call),
            Operation::ListQueueTags => self.list_queue_tags(call),
            Operation::ReceiveMessage => self.receive_message(call),
            Operation::DeleteMessage => self.delete_message(call),
            Operation::DeleteMessageBatch => self.delete_message_batch(call),
            Operation::SendMessageBatch => self.send_message_batch(call),
            Operation::ChangeMessageVisibilityBatch => self.change_message_visibility_batch(call),
            Operation::ChangeMessageVisibility => self.change_message_visibility(call),
            Operation::AddPermission
            | Operation::CancelMessageMoveTask
            | Operation::ListDeadLetterSourceQueues
            | Operation::ListMessageMoveTasks
            | Operation::RemovePermission
            | Operation::StartMessageMoveTask => Err(not_built(op)),
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
        self.queues.get_mut(name).ok_or_else(no_such_queue)
    }
}

/// An operation the model has and this build does not do yet.
fn not_built(op: Operation) -> ApiError {
    ApiError::new(501, "NotImplemented", format!("{op:?} is not built yet."))
}

/// A queue of another account, or an address that is not a queue's: AWS names the endpoint, not the URL.
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
