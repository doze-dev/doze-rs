//! Publish and PublishBatch: a message to a topic (checked, deduplicated on a FIFO topic, matched against each subscription's filter, and
//! delivered), to a phone number, or to a device endpoint. A text message and a push message are **accepted and not sent** (decision D1).

use aws_wire::ApiError;
use aws_wire::service::Caller;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::arn;
use crate::core::Core;
use crate::deliver::Published;
use crate::filter;
use crate::filter_match::{passes_attributes, passes_body};
use crate::message::{self, Message};
use crate::model::{DEDUP_WINDOW_MS, Remembered, Subscription};
use crate::req::{Req, invalid_because, not_found};

/// The most entries one `PublishBatch` takes.
const MAX_ENTRIES: usize = 10;

/// What a topic did with a message.
struct Accepted {
    message_id: String,
    sequence: Option<String>,
}

/// The reply member for a sequence number: AWS's are 20 digits, growing by a thousand.
fn sequence_number(n: u64) -> String {
    format!("1{:019}", n.saturating_mul(1000))
}

fn id() -> String {
    aws_seal::uuid_v4().unwrap_or_default()
}

/// `2026-11-12T10:00:00.123Z`: the time of a notification, to the millisecond.
fn timestamp(ms: i64) -> String {
    let whole = aws_core::time::format_iso8601(ms.div_euclid(1000));
    format!(
        "{}.{:03}Z",
        whole.trim_end_matches(".000Z"),
        ms.rem_euclid(1000)
    )
}

/// Whether a `TargetArn` names a topic (and not an application endpoint).
fn is_topic_arn(text: &str) -> bool {
    arn::split(text, "TargetArn")
        .is_ok_and(|a| a.service == "sns" && !a.resource.contains('/') && !a.resource.contains(':'))
}

impl Core {
    /// `Publish`.
    pub(crate) async fn publish(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let message = Message::from_json(&r.v);
        let target = r.opt("TargetArn").filter(|t| !t.is_empty());
        let topic_arn = r
            .opt("TopicArn")
            .filter(|t| !t.is_empty())
            .or(target.filter(|t| is_topic_arn(t)));
        if let Some(text) = topic_arn {
            let param = if r.opt("TopicArn").is_some() {
                "TopicArn"
            } else {
                "TargetArn"
            };
            let key = self.topic_key(text, param, r)?;
            let a = self.publish_one(&key, &message, &r.call.caller).await?;
            return Ok(match a.sequence {
                Some(n) => json!({"MessageId": a.message_id, "SequenceNumber": n}),
                None => json!({"MessageId": a.message_id}),
            });
        }
        if let Some(endpoint) = target {
            return self.publish_to_endpoint(endpoint, &message);
        }
        if let Some(number) = r.opt("PhoneNumber").filter(|n| !n.is_empty()) {
            return publish_to_phone(number, &message);
        }
        Err(invalid_because(
            "TopicArn or TargetArn",
            "no value for required parameter",
        ))
    }

    /// A message to a device: accepted if the endpoint is there and enabled. Nothing is sent.
    fn publish_to_endpoint(&self, endpoint: &str, m: &Message) -> Result<Value, ApiError> {
        message::check_body(m)?;
        let e = self
            .endpoints
            .get(endpoint)
            .ok_or_else(|| not_found("Endpoint does not exist"))?;
        if e.attrs.get("Enabled").map(String::as_str) == Some("false") {
            // (memory)
            return Err(ApiError::modeled(
                "EndpointDisabledException",
                "Endpoint is disabled",
            ));
        }
        Ok(json!({"MessageId": id()}))
    }

    /// Checks, records and delivers one message to topic `key`.
    async fn publish_one(
        &mut self,
        key: &str,
        m: &Message,
        caller: &Caller,
    ) -> Result<Accepted, ApiError> {
        let (accepted, subs, dedup_id) = self.accept(key, m)?;
        if !subs.is_empty() {
            let topic = self.topics.get(key).map(crate::model::Topic::light);
            if let Some(topic) = topic {
                let when = timestamp(self.now_ms());
                let published = Published {
                    topic: &topic,
                    message_id: &accepted.message_id,
                    message: m,
                    dedup_id: dedup_id.as_deref(),
                    timestamp: &when,
                    caller,
                };
                self.fan_out(&published, &subs).await;
            }
        }
        Ok(accepted)
    }

    /// The synchronous half of a publish: the checks, the FIFO bookkeeping, and who the message goes to.
    fn accept(
        &mut self,
        key: &str,
        m: &Message,
    ) -> Result<(Accepted, Vec<Subscription>, Option<String>), ApiError> {
        let now = self.now_ms();
        let topic = self.topics.get_mut(key).ok_or_else(crate::req::no_topic)?;
        message::check(topic, m)?;
        let mut accepted = Accepted {
            message_id: id(),
            sequence: None,
        };
        let mut dedup_id = None;
        if topic.fifo {
            let supplied = m.dedup_id.clone();
            let id = supplied
                .unwrap_or_else(|| aws_core::hex::encode(&Sha256::digest(m.body.as_bytes())));
            let scope = if topic.attrs.get("FifoThroughputScope").map(String::as_str)
                == Some("MessageGroup")
            {
                format!("{}/{id}", m.group_id.as_deref().unwrap_or_default())
            } else {
                id.clone()
            };
            topic.dedup.retain(|r| r.expires_ms > now);
            if let Some(seen) = topic.dedup.iter().find(|r| r.key == scope) {
                // A duplicate inside the window is accepted and not delivered again: it gets the original's answer.
                accepted.message_id.clone_from(&seen.message_id);
                accepted.sequence = Some(seen.sequence.clone());
                return Ok((accepted, Vec::new(), None));
            }
            topic.sequence += 1;
            let sequence = sequence_number(topic.sequence);
            topic.dedup.push(Remembered {
                key: scope,
                expires_ms: now + DEDUP_WINDOW_MS,
                message_id: accepted.message_id.clone(),
                sequence: sequence.clone(),
            });
            accepted.sequence = Some(sequence);
            dedup_id = Some(id);
        }
        let name = topic.name.clone();
        Ok((accepted, self.recipients(&name, m), dedup_id))
    }

    /// The confirmed subscriptions of a topic whose filter lets the message through.
    fn recipients(&self, topic: &str, m: &Message) -> Vec<Subscription> {
        let attrs = m.filterable();
        self.subs
            .iter()
            .map(|(_, s)| s)
            .filter(|s| s.topic == topic && s.confirmed)
            .filter(|s| {
                let Some(policy) = s.attrs.get("FilterPolicy") else {
                    return true;
                };
                let body = s.body_scope();
                match filter::parse(policy, body) {
                    Ok(rules) if body => passes_body(&rules, &m.text_for(&s.protocol)),
                    Ok(rules) => passes_attributes(&rules, &attrs),
                    Err(_) => false,
                }
            })
            .cloned()
            .collect()
    }

    /// `PublishBatch`: the batch is refused whole for its shape, and each message otherwise answers for itself.
    pub(crate) async fn publish_batch(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let key = self.topic_key(r.s("TopicArn"), "TopicArn", r)?;
        let entries =
            r.v.get("PublishBatchRequestEntries")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
        let max = self
            .topics
            .get(&key)
            .map_or(0, crate::model::Topic::max_size);
        check_batch(&entries, max)?;
        let (mut done, mut failed) = (Vec::new(), Vec::new());
        for entry in &entries {
            let name = entry.get("Id").and_then(Value::as_str).unwrap_or_default();
            let m = Message::from_json(entry);
            match self.publish_one(&key, &m, &r.call.caller).await {
                Ok(a) => done.push(match a.sequence {
                    Some(n) => json!({"Id": name, "MessageId": a.message_id, "SequenceNumber": n}),
                    None => json!({"Id": name, "MessageId": a.message_id}),
                }),
                Err(e) => failed.push(json!({
                    "Id": name,
                    "Code": e.code.trim_end_matches("Exception"),
                    "Message": e.message,
                    "SenderFault": true,
                })),
            }
        }
        Ok(json!({"Successful": done, "Failed": failed}))
    }
}

/// A text message: accepted, and not sent.
fn publish_to_phone(number: &str, m: &Message) -> Result<Value, ApiError> {
    message::check_body(m)?;
    if !crate::sms::is_phone_number(number) {
        // (memory)
        return Err(invalid_because(
            "PhoneNumber",
            format!("{number} is not valid to publish to"),
        ));
    }
    Ok(json!({"MessageId": id()}))
}

/// The refusals for the shape of a batch (the first three recorded 2026-11-09, the others from memory).
fn check_batch(entries: &[Value], max_size: u64) -> Result<(), ApiError> {
    if entries.is_empty() {
        return Err(ApiError::modeled(
            "EmptyBatchRequestException",
            "The batch request doesn't contain any entries.",
        ));
    }
    if entries.len() > MAX_ENTRIES {
        return Err(ApiError::modeled(
            "TooManyEntriesInBatchRequestException",
            "The batch request contains more entries than permissible.",
        ));
    }
    let ids: Vec<&str> = entries
        .iter()
        .map(|e| e.get("Id").and_then(Value::as_str).unwrap_or_default())
        .collect();
    let distinct: std::collections::BTreeSet<&&str> = ids.iter().collect();
    if distinct.len() != ids.len() {
        return Err(ApiError::modeled(
            "BatchEntryIdsNotDistinctException",
            "Two or more batch entries in the request have the same Id.",
        ));
    }
    if !ids.iter().all(|i| valid_entry_id(i)) {
        return Err(ApiError::modeled(
            "InvalidBatchEntryIdException",
            "The Id of a batch entry in a batch request doesn't abide by the specification.",
        ));
    }
    let total: usize = entries.iter().map(|e| Message::from_json(e).size()).sum();
    if total as u64 > max_size {
        return Err(ApiError::modeled(
            "BatchRequestTooLongException",
            "The length of all the messages put together is more than the limit.",
        ));
    }
    Ok(())
}

fn valid_entry_id(id: &str) -> bool {
    (1..=80).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
