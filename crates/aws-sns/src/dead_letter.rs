//! Dead-letter queues: a subscription's `RedrivePolicy` names an SQS queue (`deadLetterTargetArn`) that receives a message SNS finally
//! could not deliver, as the SNS Developer Guide's "dead-letter queues" pages describe.
//!
//! When it is sent there: for an `sqs` or `lambda` subscription, at once on a client-side error (the queue or function is gone, its
//! policy refuses SNS): AWS does not retry those. For an `http` subscription, when the delivery policy's retries are spent on a server
//! error or a 429, or at once on another client error. What is sent: the body the subscription would have been given (the JSON
//! envelope, or the raw message for a raw subscription, with the message's own attributes), plus the string attributes `ERROR_CODE`,
//! `ERROR_MESSAGE`, `RequestID` and `TopicArn` (the names and their values are (memory): not recorded). The queue's own policy must let
//! SNS send to it, as for an `sqs` subscription. A message that cannot reach the dead-letter queue either is dropped with a log line.

use std::sync::Arc;

use aws_peers::{Mode, Peers};
use aws_wire::service::Caller;
use serde_json::{Map, Value, json};

use crate::arn;
use crate::core::Core;
use crate::deliver::{Published, queue_accepts};
use crate::model::Subscription;

/// The most message attributes an SQS message carries; the message's own make room for the four SNS adds.
const SQS_MAX_ATTRIBUTES: usize = 10;

/// Why a delivery finally failed: the code and the words that go into `ERROR_CODE` and `ERROR_MESSAGE`.
#[derive(Debug)]
pub(crate) struct Failure {
    pub(crate) code: String,
    pub(crate) message: String,
}

impl Failure {
    pub(crate) fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Everything needed to send one message to a subscription's dead-letter queue, made before the delivery so an `http` delivery's
/// task (which holds no `Core`) can send it when its retries are spent.
#[derive(Debug)]
pub(crate) struct DeadLetter {
    peers: Arc<Peers>,
    caller: Caller,
    queue_arn: String,
    url: String,
    topic_arn: String,
    topic_account: String,
    subscription: String,
    /// The `SendMessage` request without the error attributes.
    body: Map<String, Value>,
}

impl Core {
    /// The dead letter for `sub`, if it has a `RedrivePolicy`. `body` is what the subscription is given; `attrs` the message's own
    /// attributes in SQS's shape (for a raw subscription).
    pub(crate) fn dead_letter_of(
        &self,
        p: &Published<'_>,
        sub: &Subscription,
        body: String,
        attrs: Option<Value>,
    ) -> Option<DeadLetter> {
        let policy: Value = serde_json::from_str(sub.attrs.get("RedrivePolicy")?).ok()?;
        let queue_arn = policy.get("deadLetterTargetArn")?.as_str()?.to_owned();
        let queue = arn::split(&queue_arn, "deadLetterTargetArn").ok()?;
        let mut caller = p.caller.clone();
        caller.region = queue.region.to_owned();
        caller.account_id = queue.account.to_owned();
        let url = format!("{}/{}/{}", p.caller.endpoint, queue.account, queue.resource);
        let mut request = Map::new();
        request.insert("QueueUrl".into(), json!(url));
        request.insert("MessageBody".into(), json!(body));
        if let Some(a) = attrs {
            request.insert("MessageAttributes".into(), a);
        }
        if p.topic.fifo {
            if let Some(g) = &p.message.group_id {
                request.insert("MessageGroupId".into(), json!(g));
            }
            if let Some(d) = p.dedup_id {
                request.insert("MessageDeduplicationId".into(), json!(d));
            }
        }
        Some(DeadLetter {
            peers: self.peers.clone()?,
            caller,
            queue_arn,
            url,
            topic_arn: p.topic.arn.clone(),
            topic_account: p.topic.account.clone(),
            subscription: sub.arn.clone(),
            body: request,
        })
    }
}

impl DeadLetter {
    /// Sends the message to the dead-letter queue with why it failed. Logs either way.
    pub(crate) async fn send(mut self, why: &Failure) {
        if self.peers.mode() != Mode::Off {
            let policy = crate::deliver::queue_policy(&self.peers, &self.caller, &self.url).await;
            if !queue_accepts(
                policy.as_deref(),
                &self.queue_arn,
                &self.topic_arn,
                &self.topic_account,
            ) {
                let refused = format!(
                    "the dead-letter queue's policy does not let {} send to it",
                    self.topic_arn
                );
                if self.peers.mode() == Mode::Enforce {
                    tracing::warn!(subscription = %self.subscription, "SNS could not move a message to its dead-letter queue: {refused}");
                    return;
                }
                tracing::warn!(
                    "iam soft mode: SNS dead-letter delivery would be refused: {refused}"
                );
            }
        }
        self.add_error_attributes(why);
        match self
            .peers
            .call(
                "sns",
                &self.caller,
                "sqs",
                "SendMessage",
                &Value::Object(self.body),
            )
            .await
        {
            Ok(_) => tracing::info!(
                subscription = %self.subscription,
                queue = %self.queue_arn,
                "SNS moved an undeliverable message to the dead-letter queue: {}: {}",
                why.code,
                why.message
            ),
            Err(e) => tracing::warn!(
                subscription = %self.subscription,
                queue = %self.queue_arn,
                "SNS could not move an undeliverable message to its dead-letter queue: {}",
                e.message
            ),
        }
    }

    fn add_error_attributes(&mut self, why: &Failure) {
        let request_id = aws_seal::uuid_v4().unwrap_or_default();
        let attrs = self
            .body
            .entry("MessageAttributes")
            .or_insert_with(|| Value::Object(Map::new()));
        let Some(attrs) = attrs.as_object_mut() else {
            return;
        };
        // The message's own attributes give way to the four SNS adds, within SQS's limit of ten.
        while attrs.len() > SQS_MAX_ATTRIBUTES - 4 {
            let Some(last) = attrs.keys().next_back().cloned() else {
                break;
            };
            attrs.remove(&last);
        }
        let string = |v: &str| json!({"DataType": "String", "StringValue": v});
        // (memory): the four names and what each holds.
        attrs.insert("ERROR_CODE".into(), string(&why.code));
        let message = if why.message.is_empty() {
            why.code.as_str()
        } else {
            why.message.as_str()
        };
        attrs.insert("ERROR_MESSAGE".into(), string(message));
        attrs.insert("RequestID".into(), string(&request_id));
        attrs.insert("TopicArn".into(), string(&self.topic_arn));
    }
}
