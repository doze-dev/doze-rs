//! Sending: one message, and a batch of them.

use aws_model_sqs::keys::{
    SendMessageBatchRequest as Batch, SendMessageBatchRequestEntry as Entry,
    SendMessageBatchResult as BatchSent, SendMessageBatchResultEntry as EntrySent,
    SendMessageRequest as Send, SendMessageResult as Sent,
};
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};

use crate::batch;
use crate::core::{Core, invalid_parameter, missing, reply, request};
use crate::md5_hex;
use crate::model::{Message, Queue};
use crate::msgattr::{self, Attr};

/// A message a caller wants sent, checked as far as the wire can check it.
pub(crate) struct Outgoing<'a> {
    pub(crate) body: &'a str,
    pub(crate) delay: Option<i64>,
    pub(crate) attrs: Vec<Attr>,
    pub(crate) system: Vec<Attr>,
    pub(crate) group_id: Option<&'a str>,
    pub(crate) dedup_id: Option<&'a str>,
}

/// What a send answers with.
pub(crate) struct Accepted {
    pub(crate) id: String,
    pub(crate) md5_of_body: String,
    pub(crate) md5_of_attrs: Option<String>,
    pub(crate) md5_of_system: Option<String>,
}

impl Queue {
    /// Puts a message in the queue.
    pub(crate) fn send(&mut self, now_ms: i64, out: &Outgoing<'_>) -> Result<Accepted, ApiError> {
        if !out.body.chars().all(allowed) {
            let bad = out.body.chars().find(|c| !allowed(*c)).unwrap_or('\0');
            return Err(ApiError::modeled(
                "InvalidMessageContents",
                format!(
                    "Invalid binary character '#x{:x}' was found in the message body, the set of allowed characters is #x9 | #xA | #xD | [#x20-#xD7FF] | [#xE000-#xFFFD] | [#x10000-#x10FFFF].",
                    bad as u32
                ),
            ));
        }
        if out.body.len() + msgattr::size_of(&out.attrs) > self.attrs.max_message_size as usize {
            return Err(invalid_parameter(format!(
                "One or more parameters are invalid. Reason: Message must be shorter than {} bytes.",
                self.attrs.max_message_size
            )));
        }
        let delay = match out.delay {
            None => self.attrs.delay_seconds,
            Some(n) if (0..=900).contains(&n) => u32::try_from(n).unwrap_or(0),
            Some(n) => {
                return Err(invalid_parameter(format!(
                    "Value {n} for parameter DelaySeconds is invalid. Reason: DelaySeconds must be >= 0 and <= 900."
                )));
            }
        };
        let accepted = Accepted {
            id: aws_wire::RequestId::new().to_string(),
            md5_of_body: md5_hex(out.body.as_bytes()),
            md5_of_attrs: msgattr::md5_of(&out.attrs),
            md5_of_system: msgattr::md5_of(&out.system),
        };
        self.messages.push(Message {
            id: accepted.id.clone(),
            body: out.body.to_owned(),
            md5_of_body: accepted.md5_of_body.clone(),
            sent_ms: now_ms,
            visible_at_ms: now_ms + i64::from(delay) * 1000,
            receive_count: 0,
            first_received_ms: None,
            handle: None,
            attributes: out.attrs.clone(),
            system: out.system.clone(),
            group_id: out.group_id.map(str::to_owned),
            dedup_id: out.dedup_id.map(str::to_owned),
        });
        Ok(accepted)
    }
}

/// The characters a message may hold (the XML 1.0 character set).
fn allowed(c: char) -> bool {
    matches!(c, '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

impl Core {
    pub(crate) fn send_message(&mut self, call: &Call) -> Reply {
        let v = request::<Send>(call)?;
        let queue_url = v.get(Send::QUEUE_URL).ok_or_else(|| missing("QueueUrl"))?;
        let out = Outgoing {
            body: v
                .get(Send::MESSAGE_BODY)
                .ok_or_else(|| missing("MessageBody"))?,
            delay: v.get(Send::DELAY_SECONDS),
            attrs: msgattr::user(v.get(Send::MESSAGE_ATTRIBUTES).into_iter().flatten())?,
            system: msgattr::system(v.get(Send::MESSAGE_SYSTEM_ATTRIBUTES).into_iter().flatten())?,
            group_id: v.get(Send::MESSAGE_GROUP_ID),
            dedup_id: v.get(Send::MESSAGE_DEDUPLICATION_ID),
        };
        let now = self.now_ms();
        let a = self.queue_mut(call, queue_url)?.send(now, &out)?;
        reply::<Sent>(|o| {
            o.put(Sent::MD5_OF_MESSAGE_BODY, &a.md5_of_body)
                .put(Sent::MESSAGE_ID, &a.id);
            if let Some(m) = &a.md5_of_attrs {
                o.put(Sent::MD5_OF_MESSAGE_ATTRIBUTES, m);
            }
            if let Some(m) = &a.md5_of_system {
                o.put(Sent::MD5_OF_MESSAGE_SYSTEM_ATTRIBUTES, m);
            }
        })
    }

    pub(crate) fn send_message_batch(&mut self, call: &Call) -> Reply {
        let v = request::<Batch>(call)?;
        let queue_url = v.get(Batch::QUEUE_URL).ok_or_else(|| missing("QueueUrl"))?;
        let entries: Vec<_> = v.get(Batch::ENTRIES).into_iter().flatten().collect();
        let ids: Vec<&str> = entries
            .iter()
            .map(|e| e.get(Entry::ID).unwrap_or(""))
            .collect();
        batch::check("SendMessageBatchRequestEntry", &ids)?;
        // Each entry is read first, so the size of the whole request is known before anything is sent.
        let parsed: Vec<_> = entries.iter().map(|e| read_entry(e)).collect();
        let total: usize = parsed
            .iter()
            .flatten()
            .map(|o| o.body.len() + msgattr::size_of(&o.attrs))
            .sum();
        if total > batch::MAX_BYTES {
            return Err(ApiError::new(
                400,
                "AWS.SimpleQueueService.BatchRequestTooLong",
                format!(
                    "Batch requests cannot be longer than {} bytes. You have sent {total} bytes.",
                    batch::MAX_BYTES
                ),
            ));
        }
        let now = self.now_ms();
        let queue = self.queue_mut(call, queue_url)?;
        let results: Vec<(&str, Result<Accepted, ApiError>)> = ids
            .iter()
            .zip(&parsed)
            .map(|(id, p)| {
                (
                    *id,
                    p.as_ref()
                        .map_err(Clone::clone)
                        .and_then(|o| queue.send(now, o)),
                )
            })
            .collect();
        reply::<BatchSent>(|o| {
            if results.iter().any(|(_, r)| r.is_ok()) {
                o.put_list(BatchSent::SUCCESSFUL, |l| {
                    for (id, a) in results
                        .iter()
                        .filter_map(|(id, r)| r.as_ref().ok().map(|a| (id, a)))
                    {
                        l.push(|e| {
                            e.put(EntrySent::ID, id)
                                .put(EntrySent::MESSAGE_ID, &a.id)
                                .put(EntrySent::MD5_OF_MESSAGE_BODY, &a.md5_of_body);
                            if let Some(m) = &a.md5_of_attrs {
                                e.put(EntrySent::MD5_OF_MESSAGE_ATTRIBUTES, m);
                            }
                            if let Some(m) = &a.md5_of_system {
                                e.put(EntrySent::MD5_OF_MESSAGE_SYSTEM_ATTRIBUTES, m);
                            }
                        });
                    }
                });
            }
            if results.iter().any(|(_, r)| r.is_err()) {
                o.put_list(BatchSent::FAILED, |l| {
                    for (id, err) in results
                        .iter()
                        .filter_map(|(id, r)| r.as_ref().err().map(|e| (id, e)))
                    {
                        l.push(|e| batch::write_failed(e, id, err));
                    }
                });
            }
        })
    }
}

/// One batch entry as an outgoing message, or the reason it cannot be one.
fn read_entry<'a>(e: &aws_schema::view::View<'a, Entry>) -> Result<Outgoing<'a>, ApiError> {
    Ok(Outgoing {
        body: e
            .get(Entry::MESSAGE_BODY)
            .ok_or_else(|| missing("MessageBody"))?,
        delay: e.get(Entry::DELAY_SECONDS),
        attrs: msgattr::user(e.get(Entry::MESSAGE_ATTRIBUTES).into_iter().flatten())?,
        system: msgattr::system(
            e.get(Entry::MESSAGE_SYSTEM_ATTRIBUTES)
                .into_iter()
                .flatten(),
        )?,
        group_id: e.get(Entry::MESSAGE_GROUP_ID),
        dedup_id: e.get(Entry::MESSAGE_DEDUPLICATION_ID),
    })
}
