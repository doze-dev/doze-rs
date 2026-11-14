//! `DozePeek`: doze's read-only look at a queue, for the console.

use aws_model_sqs::keys::ReceiveMessageRequest as Receive;
use aws_wire::service::{Call, Reply};

use crate::core::{Core, missing, request};
use crate::messages::{Taken, ranged, taken_from, write_received};

/// The most messages one `DozePeek` returns (doze's bound: the console asks for 50).
const MAX_PEEK: i64 = 100;

impl Core {
    /// `DozePeek`, doze's own operation (not AWS's): the messages a receive could be given now (not in flight, not delayed, not past
    /// their retention), every group's and not only the head of each, oldest first, read without receiving them: no receive count,
    /// no visibility timeout, no new handle. The console shows a queue with it. It reads a `ReceiveMessage` request (`QueueUrl`,
    /// `MaxNumberOfMessages`, `AttributeNames`, `MessageSystemAttributeNames`, `MessageAttributeNames`) and answers as one does;
    /// a message that was received before carries its latest handle. `MaxNumberOfMessages` is 1 to 100 (10 when absent): the bound
    /// is doze's, as the operation is.
    pub(crate) fn peek(&mut self, call: &Call) -> Reply {
        let v = request::<Receive>(call)?;
        let queue_url = v
            .get(Receive::QUEUE_URL)
            .ok_or_else(|| missing("QueueUrl"))?;
        let max = ranged(
            v.get(Receive::MAX_NUMBER_OF_MESSAGES),
            1,
            MAX_PEEK,
            10,
            "MaxNumberOfMessages",
            "Must be between 1 and 100, if provided.",
        )?;
        let system: Vec<&str> = v
            .get(Receive::MESSAGE_SYSTEM_ATTRIBUTE_NAMES)
            .into_iter()
            .flatten()
            .chain(v.get(Receive::ATTRIBUTE_NAMES).into_iter().flatten())
            .collect();
        let user: Vec<&str> = v
            .get(Receive::MESSAGE_ATTRIBUTE_NAMES)
            .into_iter()
            .flatten()
            .collect();
        let now = self.now_ms();
        let account = call.caller.account_id.clone();
        let (account_of, name) =
            crate::url::parse(queue_url).ok_or_else(|| crate::core::invalid_address(call))?;
        if account_of != account {
            return Err(crate::core::invalid_address(call));
        }
        // Read only: the queue is not marked as changed.
        let queue = self
            .queues
            .get(name)
            .ok_or_else(crate::core::no_such_queue)?;
        let taken: Vec<Taken> = queue
            .messages
            .iter()
            .filter(|m| m.visible_at_ms <= now && !queue.expired(m, now))
            .take(usize::try_from(max).unwrap_or(10))
            .map(|m| {
                let handle = m.handles.last().cloned().unwrap_or_default();
                taken_from(m, handle, (&system, &user), &account)
            })
            .collect();
        write_received(&taken)
    }
}
