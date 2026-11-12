//! Delivery: what happens to a published message at each subscription. **SQS and Lambda are delivered to** through the peer bus, as
//! SNS: a queue gets the envelope or the raw message, a function is invoked asynchronously with AWS's `Records[].Sns` event and the
//! topic as its `SourceArn`. Every other protocol is **not delivered, and not pretended to be**: the subscription is kept, the message
//! is not sent anywhere, and the log says so. That is `sms`, `application` (decision D1: stored, nothing sent), `email`, `email-json`,
//! `http` and `https` (their subscriptions wait for a confirmation token that no one is sent) and `firehose`.

use aws_iam_policy::{Context, Policies, Request, Subject};
use aws_peers::Mode;
use aws_wire::service::Caller;
use serde_json::{Map, Value, json};

use crate::arn;
use crate::core::Core;
use crate::envelope::{self, Context as Envelope};
use crate::message::{Attr, Message};
use crate::model::{Subscription, Topic};

/// One message published to a topic, as it is delivered to each subscription.
pub(crate) struct Published<'a> {
    pub(crate) topic: &'a Topic,
    pub(crate) message_id: &'a str,
    pub(crate) message: &'a Message,
    /// The deduplication id a FIFO topic delivers with: the caller's, or the hash of the body.
    pub(crate) dedup_id: Option<&'a str>,
    pub(crate) timestamp: &'a str,
    pub(crate) caller: &'a Caller,
}

impl Core {
    /// Delivers to each subscription, in turn. A delivery that fails is logged and does not stop the others.
    pub(crate) async fn fan_out(&self, p: &Published<'_>, subs: &[Subscription]) {
        for sub in subs {
            let delivered = match sub.protocol.as_str() {
                "sqs" => Some(self.to_queue(p, sub).await),
                "lambda" => Some(self.to_function(p, sub).await),
                _ => None,
            };
            if let Some(Err(why)) = delivered {
                tracing::warn!(
                    subscription = %sub.arn,
                    endpoint = %sub.endpoint,
                    "SNS could not deliver to the {}: {why}",
                    sub.protocol
                );
            } else if delivered.is_none() {
                tracing::debug!(
                    subscription = %sub.arn,
                    protocol = %sub.protocol,
                    "SNS does not deliver to this protocol locally; the message was not sent"
                );
            }
        }
    }

    async fn to_queue(&self, p: &Published<'_>, sub: &Subscription) -> Result<(), String> {
        let peers = self.peers.clone().ok_or("no queue service is running")?;
        let queue = arn::split(&sub.endpoint, "Endpoint").map_err(|e| e.message)?;
        let mut caller = p.caller.clone();
        caller.region = queue.region.to_owned();
        caller.account_id = queue.account.to_owned();
        let url = format!("{}/{}/{}", p.caller.endpoint, queue.account, queue.resource);
        if peers.mode() != Mode::Off {
            let policy = queue_policy(&peers, &caller, &url).await;
            if !queue_accepts(policy.as_deref(), &sub.endpoint, p.topic) {
                let why = format!("the queue's policy does not let {} send to it", p.topic.arn);
                if peers.mode() == Mode::Enforce {
                    return Err(why);
                }
                tracing::warn!("iam soft mode: SNS delivery would be refused: {why}");
            }
        }
        let body = self.queue_message(p, sub, &url);
        peers
            .call("sns", &caller, "sqs", "SendMessage", &body)
            .await
            .map(|_| ())
            .map_err(|e| e.message)
    }

    /// Invokes the subscribed function asynchronously with the notification event, in the function's own region and account.
    async fn to_function(&self, p: &Published<'_>, sub: &Subscription) -> Result<(), String> {
        let peers = self.peers.clone().ok_or("no function service is running")?;
        let function = arn::split(&sub.endpoint, "Endpoint").map_err(|e| e.message)?;
        let mut caller = p.caller.clone();
        caller.region = function.region.to_owned();
        caller.account_id = function.account.to_owned();
        let ctx = Envelope {
            message_id: p.message_id,
            topic_arn: &p.topic.arn,
            subscription_arn: &sub.arn,
            timestamp: p.timestamp,
            endpoint: &p.caller.endpoint,
        };
        let event = envelope::lambda_event(&ctx, p.message, &p.message.text_for("lambda"));
        let body = json!({
            "FunctionName": sub.endpoint,
            "InvocationType": "Event",
            "Payload": aws_core::base64::encode(event.as_bytes()),
            "SourceArn": p.topic.arn,
        });
        peers
            .call_rest("sns", &caller, "lambda", "Invoke", &body)
            .await
            .map(|_| ())
            .map_err(|e| e.message)
    }

    /// The `SendMessage` request for a subscription: the envelope, or for a raw subscription the message and its attributes.
    fn queue_message(&self, p: &Published<'_>, sub: &Subscription, url: &str) -> Value {
        let text = p.message.text_for("sqs");
        let mut body = Map::new();
        body.insert("QueueUrl".into(), json!(url));
        if sub.raw() {
            body.insert("MessageBody".into(), json!(text));
            if !p.message.attrs.is_empty() {
                body.insert(
                    "MessageAttributes".into(),
                    queue_attributes(&p.message.attrs),
                );
            }
        } else {
            let ctx = Envelope {
                message_id: p.message_id,
                topic_arn: &p.topic.arn,
                subscription_arn: &sub.arn,
                timestamp: p.timestamp,
                endpoint: &p.caller.endpoint,
            };
            body.insert(
                "MessageBody".into(),
                json!(envelope::notification(&ctx, p.message, &text)),
            );
        }
        if let Some(g) = &p.message.group_id {
            body.insert("MessageGroupId".into(), json!(g));
        }
        if let Some(d) = p.dedup_id.filter(|_| p.topic.fifo) {
            body.insert("MessageDeduplicationId".into(), json!(d));
        }
        Value::Object(body)
    }
}

/// The attributes of a raw delivery, in SQS's shape.
fn queue_attributes(attrs: &std::collections::BTreeMap<String, Attr>) -> Value {
    let mut out = Map::new();
    for (name, a) in attrs {
        let mut one = Map::new();
        one.insert("DataType".into(), json!(a.data_type));
        if let Some(s) = &a.string_value {
            one.insert("StringValue".into(), json!(s));
        }
        if let Some(b) = &a.binary_value {
            one.insert("BinaryValue".into(), json!(b));
        }
        out.insert(name.clone(), Value::Object(one));
    }
    Value::Object(out)
}

/// The `Policy` attribute of the queue at `url`, if it has one and can be read.
async fn queue_policy(peers: &aws_peers::Peers, caller: &Caller, url: &str) -> Option<String> {
    let asked = json!({"QueueUrl": url, "AttributeNames": ["Policy"]});
    let answer = peers
        .call("sns", caller, "sqs", "GetQueueAttributes", &asked)
        .await
        .ok()?;
    answer
        .get("Attributes")?
        .get("Policy")?
        .as_str()
        .map(str::to_owned)
}

/// Whether the queue's policy lets SNS, for this topic, send to it. SNS has no right of its own: a queue with no policy takes nothing.
fn queue_accepts(policy: Option<&str>, queue_arn: &str, topic: &Topic) -> bool {
    let Some(Ok(doc)) = policy.and_then(aws_iam_policy::parse) else {
        return false;
    };
    let mut context = Context::new();
    context
        .set("aws:SourceArn", topic.arn.clone())
        .set("aws:SourceAccount", topic.account.clone())
        .set("aws:SourceOwner", topic.account.clone())
        .set("aws:PrincipalServiceName", "sns.amazonaws.com");
    let subject = Subject {
        arns: Vec::new(),
        account: topic.account.clone(),
        service: Some("sns.amazonaws.com".to_owned()),
    };
    let resource = [&doc];
    let policies = Policies {
        resource: Some(&resource),
        ..Policies::default()
    };
    let request = Request {
        action: "sqs:SendMessage",
        resource: queue_arn,
        context: &context,
        subject: &subject,
    };
    aws_iam_policy::evaluate(&policies, &request).allowed()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn topic() -> Topic {
        Topic {
            name: "t".into(),
            arn: "arn:aws:sns:us-east-1:111122223333:t".into(),
            account: "111122223333".into(),
            fifo: false,
            created_ms: 0,
            attrs: BTreeMap::new(),
            tags: BTreeMap::new(),
            data_protection: None,
            deleted_subscriptions: 0,
            sequence: 0,
            dedup: Vec::new(),
        }
    }

    const QUEUE: &str = "arn:aws:sqs:us-east-1:111122223333:q";

    fn allow_sns(condition: &str) -> String {
        format!(
            r#"{{"Version":"2012-10-17","Statement":[{{"Effect":"Allow","Principal":{{"Service":"sns.amazonaws.com"}},"Action":"sqs:SendMessage","Resource":"{QUEUE}"{condition}}}]}}"#
        )
    }

    #[test]
    fn a_queue_takes_what_its_policy_lets_the_topic_send() {
        let t = topic();
        assert!(!queue_accepts(None, QUEUE, &t));
        assert!(queue_accepts(Some(&allow_sns("")), QUEUE, &t));
        let by_arn = allow_sns(
            r#","Condition":{"ArnEquals":{"aws:SourceArn":"arn:aws:sns:us-east-1:111122223333:t"}}"#,
        );
        assert!(queue_accepts(Some(&by_arn), QUEUE, &t));
        let other = allow_sns(
            r#","Condition":{"ArnEquals":{"aws:SourceArn":"arn:aws:sns:us-east-1:111122223333:other"}}"#,
        );
        assert!(!queue_accepts(Some(&other), QUEUE, &t));
        let anyone = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"sqs:*","Resource":"*"}]}"#;
        assert!(queue_accepts(Some(anyone), QUEUE, &t));
        let account_only = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"AWS":"111122223333"},"Action":"sqs:SendMessage","Resource":"*"}]}"#;
        assert!(!queue_accepts(Some(account_only), QUEUE, &t));
    }
}
