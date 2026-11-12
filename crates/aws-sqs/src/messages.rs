//! Receiving, deleting and re-timing messages.

use aws_model_sqs::keys::{
    ChangeMessageVisibilityRequest as Change, DeleteMessageRequest as Delete, Message as Received,
    MessageAttributeValue as UserValue, ReceiveMessageRequest as Receive,
    ReceiveMessageResult as Result_,
};
use aws_schema::build::Object;
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};

use crate::core::{Core, empty, invalid_parameter, missing, reply, request};
use crate::fifo::{DEDUP_WINDOW_MS, pick};
use crate::model::{Message, Queue};
use crate::msgattr::{self, Attr, Value};

/// The most messages one receive returns.
const MAX_RECEIVE: i64 = 10;

impl Queue {
    /// Deletes the message the receipt handle names. A handle that is well formed but names nothing succeeds: the delete is
    /// idempotent.
    pub(crate) fn delete_handle(&mut self, handle: &str) -> Result<(), ApiError> {
        if !well_formed(handle) {
            return Err(bad_handle());
        }
        self.messages
            .retain(|m| !m.handles.iter().any(|h| h == handle));
        Ok(())
    }

    /// Moves the deadline of the in-flight message the handle names to `timeout` seconds from `now_ms`.
    pub(crate) fn change_visibility(
        &mut self,
        handle: &str,
        timeout: i64,
        now_ms: i64,
    ) -> Result<(), ApiError> {
        ranged(
            Some(timeout),
            0,
            43_200,
            0,
            "VisibilityTimeout",
            "VisibilityTimeout must be an integer between 0 and 43200.",
        )?;
        if !well_formed(handle) {
            return Err(bad_handle());
        }
        // A message that is gone, or not in flight, is the same answer: the handle is of no use.
        let refused = || {
            invalid_parameter(format!(
                "Value {handle} for parameter ReceiptHandle is invalid. Reason: Message does not exist or is not available for visibility timeout change."
            ))
        };
        let m = self
            .messages
            .iter_mut()
            .find(|m| m.handles.iter().any(|h| h == handle))
            .filter(|m| m.in_flight(now_ms))
            .ok_or_else(refused)?;
        if now_ms + timeout * 1000 > m.received_ms + 43_200_000 {
            return Err(invalid_parameter(format!(
                "Value {timeout} for parameter VisibilityTimeout is invalid. Reason: Total VisibilityTimeout for the message is beyond the limit [43200 seconds]."
            )));
        }
        m.visible_at_ms = now_ms + timeout * 1000;
        Ok(())
    }
}

impl Core {
    pub(crate) fn receive_message(&mut self, call: &Call) -> Reply {
        let v = request::<Receive>(call)?;
        let queue_url = v
            .get(Receive::QUEUE_URL)
            .ok_or_else(|| missing("QueueUrl"))?;
        let ask = ReceiveAsk::read(&v)?;
        let now = self.now_ms();
        let account = call.caller.account_id.clone();
        let source = self.queue_mut(call, queue_url)?.name.clone();
        self.redrive_due(call, &source, now);
        let queue = self.queue_mut(call, queue_url)?;
        let hidden_for = ask
            .visibility
            .unwrap_or(i64::from(queue.attrs.visibility_timeout))
            * 1000;
        let keep_until = now - i64::from(queue.attrs.retention_seconds) * 1000;
        queue.messages.retain(|m| m.sent_ms > keep_until);
        let want = (ask.system.as_slice(), ask.user.as_slice());
        // A retry of a receive that carried the same attempt id gets the same messages (with fresh handles).
        let (indexes, fresh) = match retried(queue, ask.attempt, now) {
            Some(indexes) => (indexes, false),
            None => (pick(&queue.messages, now, ask.max, queue.is_fifo()), true),
        };
        let mut taken = Vec::with_capacity(indexes.len());
        for i in &indexes {
            let Some(m) = queue.messages.get_mut(*i) else {
                continue;
            };
            taken.push(if fresh {
                take(m, now, hidden_for, want, &account)
            } else {
                retake(m, want, &account)
            });
        }
        if let (Some(id), true) = (ask.attempt, queue.is_fifo()) {
            let ids = indexes
                .iter()
                .filter_map(|i| queue.messages.get(*i).map(|m| m.id.clone()))
                .collect();
            queue.fifo.attempt = Some((id.to_owned(), now, ids));
        }
        // Nothing to hand over: the caller may wait for something to arrive, for as long as it asked, or the queue's default.
        let wait = ask.wait.unwrap_or(i64::from(queue.attrs.wait_seconds));
        if taken.is_empty() && wait > 0 {
            self.poll = u32::try_from(wait).ok();
        }
        write_received(&taken)
    }

    pub(crate) fn delete_message(&mut self, call: &Call) -> Reply {
        let v = request::<Delete>(call)?;
        let queue_url = v
            .get(Delete::QUEUE_URL)
            .ok_or_else(|| missing("QueueUrl"))?;
        let handle = v
            .get(Delete::RECEIPT_HANDLE)
            .ok_or_else(|| missing("ReceiptHandle"))?;
        self.queue_mut(call, queue_url)?.delete_handle(handle)?;
        empty()
    }

    pub(crate) fn change_message_visibility(&mut self, call: &Call) -> Reply {
        let v = request::<Change>(call)?;
        let queue_url = v
            .get(Change::QUEUE_URL)
            .ok_or_else(|| missing("QueueUrl"))?;
        let handle = v
            .get(Change::RECEIPT_HANDLE)
            .ok_or_else(|| missing("ReceiptHandle"))?;
        let timeout = v
            .get(Change::VISIBILITY_TIMEOUT)
            .ok_or_else(|| missing("VisibilityTimeout"))?;
        let now = self.now_ms();
        self.queue_mut(call, queue_url)?
            .change_visibility(handle, timeout, now)?;
        empty()
    }
}

/// What a receive asked for, checked.
struct ReceiveAsk<'a> {
    max: usize,
    visibility: Option<i64>,
    system: Vec<&'a str>,
    user: Vec<&'a str>,
    attempt: Option<&'a str>,
    /// How long the caller will wait for a message, if it said.
    wait: Option<i64>,
}

impl<'a> ReceiveAsk<'a> {
    fn read(v: &aws_schema::view::View<'a, Receive>) -> Result<Self, ApiError> {
        let max = ranged(
            v.get(Receive::MAX_NUMBER_OF_MESSAGES),
            1,
            MAX_RECEIVE,
            1,
            "MaxNumberOfMessages",
            "Must be between 1 and 10, if provided.",
        )?;
        let visibility = v.get(Receive::VISIBILITY_TIMEOUT);
        if let Some(n) = visibility {
            ranged(
                Some(n),
                0,
                43_200,
                0,
                "VisibilityTimeout",
                "Must be between 0 and 43200, if provided.",
            )?;
        }
        let wait = v.get(Receive::WAIT_TIME_SECONDS);
        ranged(
            wait,
            0,
            20,
            0,
            "WaitTimeSeconds",
            "Must be >= 0 and <= 20, if provided.",
        )?;
        Ok(Self {
            max: usize::try_from(max).unwrap_or(1),
            visibility,
            system: v
                .get(Receive::MESSAGE_SYSTEM_ATTRIBUTE_NAMES)
                .into_iter()
                .flatten()
                .chain(v.get(Receive::ATTRIBUTE_NAMES).into_iter().flatten())
                .collect(),
            user: v
                .get(Receive::MESSAGE_ATTRIBUTE_NAMES)
                .into_iter()
                .flatten()
                .collect(),
            attempt: v.get(Receive::RECEIVE_REQUEST_ATTEMPT_ID),
            wait,
        })
    }
}

/// A message as it was handed to a receiver.
struct Taken {
    id: String,
    handle: String,
    md5_of_body: String,
    body: String,
    system: Vec<(String, String)>,
    user: Vec<Attr>,
}

/// The messages a retry of an earlier receive should return, if `attempt` is that receive's id and the window has not passed.
fn retried(queue: &Queue, attempt: Option<&str>, now_ms: i64) -> Option<Vec<usize>> {
    let id = attempt.filter(|_| queue.is_fifo())?;
    let (earlier, at, ids) = queue.fifo.attempt.as_ref()?;
    if earlier != id || now_ms - at >= DEDUP_WINDOW_MS {
        return None;
    }
    let indexes: Vec<usize> = queue
        .messages
        .iter()
        .enumerate()
        .filter(|(_, m)| ids.contains(&m.id))
        .map(|(i, _)| i)
        .collect();
    // Only while they are all still in flight: if one was deleted or came back, this is a new receive.
    (!indexes.is_empty()
        && indexes
            .iter()
            .all(|i| queue.messages.get(*i).is_some_and(|m| m.in_flight(now_ms))))
    .then_some(indexes)
}

/// Hands `m` out again for a retry: a new handle, and nothing else about it changes.
fn retake(m: &mut Message, want: (&[&str], &[&str]), account: &str) -> Taken {
    let handle = new_handle();
    m.handles.push(handle.clone());
    taken_from(m, handle, want, account)
}

/// Marks `m` received and hides it for `hidden_for` milliseconds. `want` is the system and the user attribute names asked for.
fn take(
    m: &mut Message,
    now: i64,
    hidden_for: i64,
    want: (&[&str], &[&str]),
    account: &str,
) -> Taken {
    m.receive_count += 1;
    m.first_received_ms.get_or_insert(now);
    m.visible_at_ms = now + hidden_for;
    let handle = new_handle();
    m.received_ms = now;
    m.handles.push(handle.clone());
    taken_from(m, handle, want, account)
}

fn taken_from(m: &Message, handle: String, want: (&[&str], &[&str]), account: &str) -> Taken {
    Taken {
        id: m.id.clone(),
        handle,
        md5_of_body: m.md5_of_body.clone(),
        body: m.body.clone(),
        system: system_attributes(m, want.0, account),
        user: msgattr::select(&m.attributes, want.1)
            .into_iter()
            .cloned()
            .collect(),
    }
}

fn write_received(taken: &[Taken]) -> Reply {
    reply::<Result_>(|o| {
        if taken.is_empty() {
            return;
        }
        o.put_list(Result_::MESSAGES, |list| {
            for t in taken {
                list.push(|m| write_message(m, t));
            }
        });
    })
}

fn write_message(m: &mut Object<'_, Received>, t: &Taken) {
    m.put(Received::MESSAGE_ID, &t.id)
        .put(Received::RECEIPT_HANDLE, &t.handle)
        .put(Received::MD5_OF_BODY, &t.md5_of_body)
        .put(Received::BODY, &t.body);
    if !t.system.is_empty() {
        m.put_map(Received::ATTRIBUTES, |a| {
            for (name, value) in &t.system {
                a.insert(name, value);
            }
        });
    }
    if let Some(md5) = msgattr::md5_of(&t.user) {
        m.put(Received::MD5_OF_MESSAGE_ATTRIBUTES, &md5);
        m.put_map(Received::MESSAGE_ATTRIBUTES, |map| {
            for a in &t.user {
                map.insert(&a.name, |v| {
                    v.put(UserValue::DATA_TYPE, &a.data_type);
                    match &a.value {
                        Value::Text(text) => v.put(UserValue::STRING_VALUE, text),
                        Value::Binary(bytes) => v.put_blob(UserValue::BINARY_VALUE, bytes),
                    };
                });
            }
        });
    }
}

/// `value` if it is in `lo..=hi`, `default` if absent, else the error AWS gives.
fn ranged(
    value: Option<i64>,
    lo: i64,
    hi: i64,
    default: i64,
    parameter: &str,
    reason: &str,
) -> Result<i64, ApiError> {
    match value {
        None => Ok(default),
        Some(n) if (lo..=hi).contains(&n) => Ok(n),
        Some(n) => Err(invalid_parameter(format!(
            "Value {n} for parameter {parameter} is invalid. Reason: {reason}"
        ))),
    }
}

/// A receipt handle: 48 hex digits. AWS's are longer and opaque; what matters is that a client cannot make one up.
pub(crate) fn new_handle() -> String {
    let mut bytes = [0u8; 24];
    // WHY: with no randomness the handle is all zeros; a collision costs one wrong-message delete in a test environment that has
    // no entropy source, which is not a place anyone runs this.
    if getrandom::fill(&mut bytes).is_err() {
        bytes = [0; 24];
    }
    aws_core::hex::encode(&bytes)
}

fn well_formed(handle: &str) -> bool {
    handle.len() == 48 && handle.bytes().all(|b| b.is_ascii_hexdigit())
}

fn bad_handle() -> ApiError {
    ApiError::modeled(
        "ReceiptHandleIsInvalid",
        "The input receipt handle is invalid.",
    )
}

/// The system attributes of `m` the caller asked for by name (or `All`).
fn system_attributes(m: &Message, want: &[&str], account: &str) -> Vec<(String, String)> {
    let all = want.contains(&"All");
    let mut out = Vec::new();
    let mut put = |name: &str, value: Option<String>| {
        if let Some(value) = value.filter(|_| all || want.contains(&name)) {
            out.push((name.to_owned(), value));
        }
    };
    put("SenderId", Some(account.to_owned()));
    put("SentTimestamp", Some(m.sent_ms.to_string()));
    put("ApproximateReceiveCount", Some(m.receive_count.to_string()));
    put(
        "ApproximateFirstReceiveTimestamp",
        Some(m.first_received_ms.unwrap_or(0).to_string()),
    );
    put(
        "AWSTraceHeader",
        m.system
            .iter()
            .find(|a| a.name == "AWSTraceHeader")
            .and_then(|a| text(&a.value)),
    );
    put("MessageGroupId", m.group_id.clone());
    put("MessageDeduplicationId", m.dedup_id.clone());
    put("SequenceNumber", m.sequence.clone());
    put("DeadLetterQueueSourceArn", m.source_arn.clone());
    out
}

fn text(v: &Value) -> Option<String> {
    match v {
        Value::Text(t) => Some(t.clone()),
        Value::Binary(_) => None,
    }
}
