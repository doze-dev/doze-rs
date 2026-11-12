//! The operations on messages.

use aws_model_sqs::keys::{
    ChangeMessageVisibilityRequest as Change, DeleteMessageRequest as Delete, Message as Received,
    ReceiveMessageRequest as Receive, ReceiveMessageResult as Received_,
    SendMessageRequest as Send, SendMessageResult as Sent,
};
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};
use md5::{Digest, Md5};

use crate::core::{Core, empty, invalid_parameter, missing, reply, request};
use crate::model::Message;

/// The most messages one receive returns.
const MAX_RECEIVE: i64 = 10;

impl Core {
    pub(crate) fn send_message(&mut self, call: &Call) -> Reply {
        let v = request::<Send>(call)?;
        let queue_url = v.get(Send::QUEUE_URL).ok_or_else(|| missing("QueueUrl"))?;
        let body = v
            .get(Send::MESSAGE_BODY)
            .ok_or_else(|| missing("MessageBody"))?;
        let delay = v.get(Send::DELAY_SECONDS);
        let now = self.now_ms();
        let queue = self.queue_mut(call, queue_url)?;
        if !body.chars().all(allowed) {
            return Err(ApiError::modeled(
                "InvalidMessageContents",
                "Invalid characters found. Valid unicode characters are #x9 | #xA | #xD | #x20 to #xD7FF | #xE000 to #xFFFD | #x10000 to #x10FFFF",
            ));
        }
        if body.len() > queue.attrs.max_message_size as usize {
            return Err(invalid_parameter(format!(
                "One or more parameters are invalid. Reason: Message must be shorter than {} bytes.",
                queue.attrs.max_message_size
            )));
        }
        let delay = match delay {
            None => queue.attrs.delay_seconds,
            Some(n) if (0..=900).contains(&n) => u32::try_from(n).unwrap_or(0),
            Some(n) => {
                return Err(invalid_parameter(format!(
                    "Value {n} for parameter DelaySeconds is invalid. Reason: DelaySeconds must be >= 0 and <= 900."
                )));
            }
        };
        let id = aws_wire::RequestId::new().to_string();
        let md5 = md5_hex(body.as_bytes());
        queue.messages.push(Message {
            id: id.clone(),
            body: body.to_owned(),
            md5_of_body: md5.clone(),
            sent_ms: now,
            visible_at_ms: now + i64::from(delay) * 1000,
            receive_count: 0,
            first_received_ms: None,
            handle: None,
        });
        reply::<Sent>(|o| {
            o.put(Sent::MD5_OF_MESSAGE_BODY, &md5)
                .put(Sent::MESSAGE_ID, &id);
        })
    }

    pub(crate) fn receive_message(&mut self, call: &Call) -> Reply {
        let v = request::<Receive>(call)?;
        let queue_url = v
            .get(Receive::QUEUE_URL)
            .ok_or_else(|| missing("QueueUrl"))?;
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
        ranged(
            v.get(Receive::WAIT_TIME_SECONDS),
            0,
            20,
            0,
            "WaitTimeSeconds",
            "Must be >= 0 and <= 20, if provided.",
        )?;
        let want: Vec<&str> = v
            .get(Receive::MESSAGE_SYSTEM_ATTRIBUTE_NAMES)
            .into_iter()
            .flatten()
            .chain(v.get(Receive::ATTRIBUTE_NAMES).into_iter().flatten())
            .collect();
        let now = self.now_ms();
        let account = call.caller.account_id.clone();
        let queue = self.queue_mut(call, queue_url)?;
        let hidden_for = visibility.unwrap_or(i64::from(queue.attrs.visibility_timeout)) * 1000;
        let keep_until = now - i64::from(queue.attrs.retention_seconds) * 1000;
        queue.messages.retain(|m| m.sent_ms > keep_until);
        let limit = usize::try_from(max).unwrap_or(1);
        let taken: Vec<Taken> = queue
            .messages
            .iter_mut()
            .filter(|m| m.visible_at_ms <= now)
            .take(limit)
            .map(|m| take(m, now, hidden_for, &want, &account))
            .collect();
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
        let queue = self.queue_mut(call, queue_url)?;
        if !well_formed(handle) {
            return Err(bad_handle());
        }
        // Deleting what is not there succeeds: the delete is idempotent.
        queue
            .messages
            .retain(|m| m.handle.as_deref() != Some(handle));
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
        ranged(
            Some(timeout),
            0,
            43_200,
            0,
            "VisibilityTimeout",
            "Must be between 0 and 43200, if provided.",
        )?;
        let now = self.now_ms();
        let queue = self.queue_mut(call, queue_url)?;
        if !well_formed(handle) {
            return Err(bad_handle());
        }
        let Some(m) = queue
            .messages
            .iter_mut()
            .find(|m| m.handle.as_deref() == Some(handle))
        else {
            return Err(ApiError::modeled(
                "MessageNotInflight",
                "Message does not exist or is not available for visibility timeout change.",
            ));
        };
        if !m.in_flight(now) {
            return Err(ApiError::modeled(
                "MessageNotInflight",
                "Message is not in flight.",
            ));
        }
        m.visible_at_ms = now + timeout * 1000;
        empty()
    }
}

/// A message as it was handed to a receiver.
struct Taken {
    id: String,
    handle: String,
    md5_of_body: String,
    body: String,
    attributes: Vec<(String, String)>,
}

/// Marks `m` received and hides it for `hidden_for` milliseconds.
fn take(m: &mut Message, now: i64, hidden_for: i64, want: &[&str], account: &str) -> Taken {
    m.receive_count += 1;
    m.first_received_ms.get_or_insert(now);
    m.visible_at_ms = now + hidden_for;
    let handle = new_handle();
    m.handle = Some(handle.clone());
    Taken {
        id: m.id.clone(),
        handle,
        md5_of_body: m.md5_of_body.clone(),
        body: m.body.clone(),
        attributes: system_attributes(m, want, account),
    }
}

fn write_received(taken: &[Taken]) -> Reply {
    reply::<Received_>(|o| {
        if taken.is_empty() {
            return;
        }
        o.put_list(Received_::MESSAGES, |list| {
            for t in taken {
                list.push(|m| {
                    m.put(Received::MESSAGE_ID, &t.id)
                        .put(Received::RECEIPT_HANDLE, &t.handle)
                        .put(Received::MD5_OF_BODY, &t.md5_of_body)
                        .put(Received::BODY, &t.body);
                    if !t.attributes.is_empty() {
                        m.put_map(Received::ATTRIBUTES, |a| {
                            for (name, value) in &t.attributes {
                                a.insert(name, value);
                            }
                        });
                    }
                });
            }
        });
    })
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

/// The characters a message may hold (the XML 1.0 character set).
fn allowed(c: char) -> bool {
    matches!(c, '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

fn md5_hex(bytes: &[u8]) -> String {
    aws_core::hex::encode(&Md5::digest(bytes))
}

/// A receipt handle: 48 hex digits. AWS's are longer and opaque; what matters is that a client cannot make one up.
fn new_handle() -> String {
    let mut bytes = [0u8; 24];
    // WHY: with no randomness the handle is all zeros; it is still unique among live handles because the message id is not
    // part of it only by chance, and a collision costs one wrong-message delete in a test environment without an entropy source.
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
    let mut put = |name: &str, value: String| {
        if all || want.contains(&name) {
            out.push((name.to_owned(), value));
        }
    };
    put("SenderId", account.to_owned());
    put("SentTimestamp", m.sent_ms.to_string());
    put("ApproximateReceiveCount", m.receive_count.to_string());
    put(
        "ApproximateFirstReceiveTimestamp",
        m.first_received_ms.unwrap_or(0).to_string(),
    );
    out
}
