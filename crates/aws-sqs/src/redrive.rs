//! Dead-letter queues: the redrive policies that name them, the move that happens when a message is received too often, and
//! the queues that name a given one.
//!
//! The messages are those AWS gave in `test_redrive_*`, `test_an_arn_elsewhere` and `test_dead_letter_queue_takes_what_was_received_too_often`
//! (recorded 2026-11-09). A message is moved on the receive *after* its count reaches `maxReceiveCount`, keeps its count, and
//! gains `DeadLetterQueueSourceArn`. Unrecorded wording is marked.

use aws_model_sqs::keys::{
    ListDeadLetterSourceQueuesRequest as List, ListDeadLetterSourceQueuesResult as Listed,
};
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};
use serde_json::{Value, json};

use crate::attrs::Requested;
use crate::core::{Core, invalid_parameter, missing, reply, request};
use crate::model::{Message, Queue};
use crate::policy::{self, arn};
use crate::url;

/// A queue's redrive policy as stored: where dead letters go, and after how many receives.
pub(crate) struct Redrive {
    pub(crate) target_arn: String,
    pub(crate) max_receive_count: u32,
}

impl Queue {
    /// This queue's redrive policy, if it has one.
    pub(crate) fn redrive(&self) -> Option<Redrive> {
        let doc: Value = serde_json::from_str(self.stored.get("RedrivePolicy")?).ok()?;
        Some(Redrive {
            target_arn: doc.get("deadLetterTargetArn")?.as_str()?.to_owned(),
            max_receive_count: u32::try_from(doc.get("maxReceiveCount")?.as_u64()?).ok()?,
        })
    }
}

/// The queue name an SQS ARN ends in, if it is one in `account`.
fn target_name(arn: &str) -> Option<&str> {
    arn.rsplit(':').next()
}

fn bad_allow_policy() -> ApiError {
    // AWS's message has a straight apostrophe in "can't" and a curly one in "it\u{2019}s".
    ApiError::new(
        400,
        "InvalidAttributeValue",
        "Invalid value for the parameter RedriveAllowPolicy. Reason: Amazon SQS can't create the redrive allow policy, as it\u{2019}s in an unsupported format.",
    )
}

/// Checks a `RedriveAllowPolicy`: a permission of `allowAll`, `denyAll` or `byQueue`, and for `byQueue` the queues allowed.
fn check_allow_policy(raw: &str) -> Result<(), ApiError> {
    let doc: Value = serde_json::from_str(raw).map_err(|_| bad_allow_policy())?;
    match doc.get("redrivePermission").and_then(Value::as_str) {
        Some("allowAll" | "denyAll") => Ok(()),
        Some("byQueue") => {
            let named = doc
                .get("sourceQueueArns")
                .and_then(Value::as_array)
                .is_some_and(|a| !a.is_empty());
            if named {
                Ok(())
            } else {
                Err(invalid_parameter(format!(
                    "Value {raw} for parameter RedriveAllowPolicy is invalid. Reason: Amazon SQS can't create the redrive allow policy. When you specify the byQueue permission type, you must also specify one or more sourceQueueArns values."
                )))
            }
        }
        _ => Err(bad_allow_policy()),
    }
}

impl Core {
    /// Checks the documents among the attributes a caller set on queue `source`: its policy, its redrive policy (which also
    /// reads the dead-letter queue), and its redrive allow policy. The redrive policy is stored normalised.
    pub(crate) fn check_documents(
        &self,
        call: &Call,
        source: &str,
        asked: &mut Requested,
    ) -> Result<(), ApiError> {
        if let Some(p) = asked.stored.get("Policy") {
            policy::validate(p)?;
        }
        if let Some(p) = asked.stored.get("RedriveAllowPolicy") {
            check_allow_policy(p)?;
        }
        if let Some(raw) = asked.stored.get("RedrivePolicy").cloned() {
            let normal = self.check_redrive(call, source, &raw)?;
            asked.stored.insert("RedrivePolicy".to_owned(), normal);
        }
        Ok(())
    }

    fn check_redrive(&self, call: &Call, source: &str, raw: &str) -> Result<String, ApiError> {
        let bad = |reason: &str| {
            invalid_parameter(format!(
                "Value {raw} for parameter RedrivePolicy is invalid. Reason: {reason}"
            ))
        };
        let doc: Value = serde_json::from_str(raw)
            .map_err(|_| bad("Redrive policy is not a valid JSON document."))?;
        let target = doc
            .get("deadLetterTargetArn")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                bad("Redrive policy does not contain mandatory attribute: deadLetterTargetArn.")
            })?;
        let count = match doc.get("maxReceiveCount") {
            Some(Value::String(s)) => s.parse::<i64>().ok(),
            Some(Value::Number(n)) => n.as_i64(),
            _ => {
                return Err(bad(
                    "Redrive policy does not contain mandatory attribute: maxReceiveCount.",
                ));
            }
        };
        let shown = doc
            .get("maxReceiveCount")
            .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
            .unwrap_or_default();
        let count = count
            .filter(|n| (1..=1000).contains(n))
            .ok_or_else(|| bad(&format!("Invalid value for maxReceiveCount: {shown}, valid values are from 1 to 1000 both inclusive.")))?;
        let parts: Vec<&str> = target.split(':').collect();
        let [_, _, "sqs", _, account, name] = parts.as_slice() else {
            return Err(bad(
                "Only SQS queues are valid resources for deadLetterTargetArn.",
            ));
        };
        if *account != call.caller.account_id {
            return Err(bad(
                "Dead-letter target owner should be same as the source.",
            ));
        }
        let dead_letters = self
            .queues
            .get(*name)
            .ok_or_else(|| bad("Dead letter target does not exist."))?;
        let source_arn = arn(&call.caller.region, &call.caller.account_id, source);
        if !allows(dead_letters, &source_arn) {
            return Err(invalid_parameter(format!(
                "Value RedrivePolicy(maxReceiveCount={count}, deadLetterTargetArn={target}) for parameter RedrivePolicy is invalid. Reason: The value for the dead-letter queue's RedriveAllowPolicy attribute prevents you from completing this request."
            )));
        }
        Ok(json!({"deadLetterTargetArn": target, "maxReceiveCount": count}).to_string())
    }

    /// Moves the messages of `source` that have been received as often as its redrive policy allows to its dead-letter queue.
    /// Called when the queue is received from, because that is when AWS notices.
    pub(crate) fn redrive_due(&mut self, call: &Call, source: &str, now_ms: i64) {
        let Some(policy) = self.queues.get(source).and_then(Queue::redrive) else {
            return;
        };
        let Some(target) = target_name(&policy.target_arn).map(str::to_owned) else {
            return;
        };
        if target == source || !self.queues.contains_key(&target) {
            return;
        }
        let source_arn = arn(&call.caller.region, &call.caller.account_id, source);
        let Some(queue) = self.queues.get_mut(source) else {
            return;
        };
        let (due, keep): (Vec<Message>, Vec<Message>) = std::mem::take(&mut queue.messages)
            .into_iter()
            .partition(|m| {
                m.visible_at_ms <= now_ms && m.receive_count >= policy.max_receive_count
            });
        queue.messages = keep;
        if let Some(dead_letters) = self.queues.get_mut(&target) {
            for mut m in due {
                m.handles.clear();
                m.visible_at_ms = now_ms;
                m.source_arn = Some(source_arn.clone());
                dead_letters.messages.push(m);
            }
        }
    }

    pub(crate) fn list_dead_letter_source_queues(&mut self, call: &Call) -> Reply {
        let v = request::<List>(call)?;
        let queue_url = v.get(List::QUEUE_URL).ok_or_else(|| missing("QueueUrl"))?;
        let target = self.arn_of(call, queue_url)?;
        let after = v
            .get(List::NEXT_TOKEN)
            .map(|t| crate::queues::decode_token(t, ""))
            .transpose()?;
        let limit = match v.get(List::MAX_RESULTS) {
            None => usize::MAX,
            Some(n) if (1..=1000).contains(&n) => usize::try_from(n).unwrap_or(1000),
            Some(_) => {
                return Err(invalid_parameter(
                    "Value for parameter MaxResults is invalid. Reason: MaxResults must be an integer between 1 and 1000.",
                ));
            }
        };
        let mut sources: Vec<&str> = self
            .queues
            .values()
            .filter(|q| q.redrive().is_some_and(|r| r.target_arn == target))
            .map(|q| q.name.as_str())
            .filter(|n| after.as_deref().is_none_or(|a| *n > a))
            .collect();
        let more = sources.len() > limit;
        sources.truncate(limit);
        let next = more
            .then(|| {
                sources
                    .last()
                    .map(|last| crate::queues::encode_token("", last))
            })
            .flatten();
        let urls: Vec<String> = sources
            .iter()
            .map(|n| url::build(&call.caller.endpoint, &call.caller.account_id, n))
            .collect();
        reply::<Listed>(|o| {
            o.put_list(Listed::QUEUE_URLS, |l| {
                for u in &urls {
                    l.push(u);
                }
            });
            if let Some(token) = &next {
                o.put(Listed::NEXT_TOKEN, token);
            }
        })
    }
}

/// Whether `dead_letters`'s redrive allow policy lets the queue `source_arn` name it as its dead-letter queue.
fn allows(dead_letters: &Queue, source_arn: &str) -> bool {
    let Some(raw) = dead_letters.stored.get("RedriveAllowPolicy") else {
        return true;
    };
    let Ok(doc) = serde_json::from_str::<Value>(raw) else {
        return true;
    };
    match doc.get("redrivePermission").and_then(Value::as_str) {
        Some("denyAll") => false,
        Some("byQueue") => doc
            .get("sourceQueueArns")
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().any(|x| x.as_str() == Some(source_arn))),
        _ => true,
    }
}
