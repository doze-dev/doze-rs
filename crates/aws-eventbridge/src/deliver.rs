//! Sending an event to one target, through the peer bus. Local targets: SQS, SNS, Kinesis, CloudWatch Logs, another event bus in the
//! same region, and (once those services run) Step Functions and Lambda. A target on any other service is stored and **not delivered**:
//! the log says so, and nothing is faked.
//!
//! A target with a `RoleArn` is delivered as that role would be, and the role is not checked (the evaluator runs on the caller, not on
//! roles). A target without one is delivered as `events.amazonaws.com` for the rule, and the target's own resource policy (an SQS
//! queue's, an SNS topic's) is asked, as on AWS, with `aws:SourceArn` the rule and `aws:SourceAccount` its account (the SQS scenario
//! of the conformance suite sets such a policy; the condition keys are documented).

use aws_peers::Mode;
use aws_wire::ApiError;
use aws_wire::service::Caller;
use serde_json::{Value, json};

use crate::core::Core;
use crate::event::Event;
use crate::model::StoredTarget;
use crate::names::{region_and_account, service_of};
use crate::policy::allows;

/// An event matched to one target of one rule, with what the target is to be sent.
#[derive(Clone, Debug)]
pub(crate) struct Delivery {
    pub(crate) rule_arn: String,
    pub(crate) rule_name: String,
    pub(crate) target: StoredTarget,
    /// The target as the caller sent it.
    pub(crate) spec: Value,
    /// What the target is sent: the event, or what its input settings made of it.
    pub(crate) payload: String,
    pub(crate) event: Event,
}

/// What came of sending.
#[derive(Debug)]
pub(crate) enum Outcome {
    Sent,
    /// The target is an event bus of this region: the event goes on to it.
    Forward {
        account: String,
        bus: String,
    },
    /// The target is one nothing here can deliver to; it was stored and is skipped.
    NotLocal(String),
    Failed(Failure),
}

/// Why a delivery failed, and whether trying again could help.
#[derive(Clone, Debug)]
pub(crate) struct Failure {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) retryable: bool,
}

impl Failure {
    fn of(e: &ApiError) -> Outcome {
        // A server fault or throttling may pass; anything the target refused as wrong will not.
        let retryable = e.status >= 500 || e.code.contains("Throttl") || e.code.contains("Limit");
        Outcome::Failed(Self {
            code: e.code.to_string(),
            message: e.message.clone(),
            retryable,
        })
    }

    fn fatal(code: &str, message: impl Into<String>) -> Outcome {
        Outcome::Failed(Self {
            code: code.to_owned(),
            message: message.into(),
            retryable: false,
        })
    }
}

impl Core {
    /// Sends the delivery to its target.
    pub(crate) async fn send(&self, d: &Delivery, caller: &Caller) -> Outcome {
        let arn = d.target.arn.as_str();
        let service = service_of(arn);
        let (region, account) = region_and_account(arn);
        match service {
            "sqs" => self.to_queue(d, caller).await,
            "sns" => self.to_topic(d, caller).await,
            "kinesis" => self.to_stream(d, caller).await,
            "logs" => self.to_log_group(d, caller).await,
            "states" => {
                self.to_peer(
                    d,
                    caller,
                    "states",
                    "StartExecution",
                    json!({"stateMachineArn": arn, "input": d.payload}),
                )
                .await
            }
            "lambda" => {
                let body = json!({"FunctionName": arn, "InvocationType": "Event", "Payload": aws_core::base64::encode(d.payload.as_bytes())});
                self.to_peer(d, caller, "lambda", "Invoke", body).await
            }
            "events" => match crate::names::Parts::of(arn) {
                Some(p) if p.kind == "event-bus" && p.region == caller.region => Outcome::Forward {
                    account: account.to_owned(),
                    bus: p.tail.to_owned(),
                },
                Some(p) if p.kind == "event-bus" => {
                    let _ = region;
                    Outcome::NotLocal(format!(
                        "event buses of {} are in another region's state",
                        p.region
                    ))
                }
                _ => Outcome::NotLocal("this events target is not an event bus".to_owned()),
            },
            other => Outcome::NotLocal(format!(
                "targets on {} are stored and not delivered locally",
                if other.is_empty() {
                    "this service"
                } else {
                    other
                }
            )),
        }
    }

    /// A call to a peer service as EventBridge; a peer that is not running is a target nothing delivers to.
    async fn to_peer(
        &self,
        d: &Delivery,
        caller: &Caller,
        service: &str,
        op: &str,
        body: Value,
    ) -> Outcome {
        let Some(peers) = self.peers.clone() else {
            return Outcome::NotLocal(format!(
                "{service} is not running, so the target is stored and not delivered locally"
            ));
        };
        match peers.call("events", caller, service, op, &body).await {
            Ok(_) => Outcome::Sent,
            Err(e) if e.message.starts_with("no service") => Outcome::NotLocal(format!(
                "{service} is not running, so target {} is stored and not delivered locally",
                d.target.id
            )),
            Err(e) => Failure::of(&e),
        }
    }

    async fn to_queue(&self, d: &Delivery, caller: &Caller) -> Outcome {
        let Some(peers) = self.peers.clone() else {
            return Outcome::NotLocal("no queue service is running".to_owned());
        };
        let (region, account) = region_and_account(&d.target.arn);
        let name = d.target.arn.rsplit(':').next().unwrap_or_default();
        let mut as_queue = caller.clone();
        as_queue.region = region.to_owned();
        as_queue.account_id = account.to_owned();
        let asked = json!({"QueueName": name, "QueueOwnerAWSAccountId": account});
        let url = match peers
            .call("events", &as_queue, "sqs", "GetQueueUrl", &asked)
            .await
        {
            Ok(v) => v
                .get("QueueUrl")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            Err(e) => return Failure::of(&e),
        };
        if let Some(refused) = self.target_policy(d, &peers, &as_queue, "sqs", &url).await {
            return refused;
        }
        let mut body = serde_json::Map::new();
        body.insert("QueueUrl".into(), json!(url));
        body.insert("MessageBody".into(), json!(d.payload));
        if let Some(group) = d
            .spec
            .pointer("/SqsParameters/MessageGroupId")
            .and_then(Value::as_str)
        {
            body.insert("MessageGroupId".into(), json!(group));
            // (memory) a FIFO queue needs a deduplication id when it has no content-based one; the event's id is a fit one.
            body.insert("MessageDeduplicationId".into(), json!(d.event.id));
        }
        let body = Value::Object(body);
        match peers
            .call("events", &as_queue, "sqs", "SendMessage", &body)
            .await
        {
            Ok(_) => Outcome::Sent,
            Err(e) => Failure::of(&e),
        }
    }

    async fn to_topic(&self, d: &Delivery, caller: &Caller) -> Outcome {
        let Some(peers) = self.peers.clone() else {
            return Outcome::NotLocal("no topic service is running".to_owned());
        };
        let (region, account) = region_and_account(&d.target.arn);
        let mut as_topic = caller.clone();
        as_topic.region = region.to_owned();
        as_topic.account_id = account.to_owned();
        if let Some(refused) = self
            .target_policy(d, &peers, &as_topic, "sns", &d.target.arn)
            .await
        {
            return refused;
        }
        let body = json!({"TopicArn": d.target.arn, "Message": d.payload});
        match peers
            .call("events", &as_topic, "sns", "Publish", &body)
            .await
        {
            Ok(_) => Outcome::Sent,
            Err(e) => Failure::of(&e),
        }
    }

    async fn to_stream(&self, d: &Delivery, caller: &Caller) -> Outcome {
        // The partition key: the member at `PartitionKeyPath` if there is one, else the event's id (documented).
        let key = d
            .spec
            .pointer("/KinesisParameters/PartitionKeyPath")
            .and_then(Value::as_str)
            .and_then(|p| crate::jsonpath::select(&d.event.doc(), p))
            .map_or_else(
                || d.event.id.clone(),
                |v| v.as_str().map_or_else(|| v.to_string(), str::to_owned),
            );
        let body = json!({
            "StreamARN": d.target.arn,
            "Data": aws_core::base64::encode(d.payload.as_bytes()),
            "PartitionKey": key,
        });
        self.to_peer(d, caller, "kinesis", "PutRecord", body).await
    }

    async fn to_log_group(&self, d: &Delivery, caller: &Caller) -> Outcome {
        let Some(rest) = d.target.arn.split_once(":log-group:").map(|(_, g)| g) else {
            return Outcome::NotLocal("a logs target is a log group".to_owned());
        };
        let group = rest.trim_end_matches(":*");
        let Some(peers) = self.peers.clone() else {
            return Outcome::NotLocal("no logs service is running".to_owned());
        };
        // The stream is named for the rule (memory); creating it again is not an error that matters.
        let stream = json!({"logGroupName": group, "logStreamName": d.rule_name});
        if let Err(e) = peers
            .call("events", caller, "logs", "CreateLogStream", &stream)
            .await
            && e.code != "ResourceAlreadyExistsException"
        {
            return Failure::of(&e);
        }
        let body = json!({
            "logGroupName": group, "logStreamName": d.rule_name,
            "logEvents": [{"timestamp": self.now_ms(), "message": d.payload}],
        });
        match peers
            .call("events", caller, "logs", "PutLogEvents", &body)
            .await
        {
            Ok(_) => Outcome::Sent,
            Err(e) => Failure::of(&e),
        }
    }

    /// Asks the target's resource policy whether `events.amazonaws.com` may deliver for this rule. `Some` is a refusal. A target with a
    /// role is not asked, and with `--iam off` nothing is.
    async fn target_policy(
        &self,
        d: &Delivery,
        peers: &aws_peers::Peers,
        caller: &Caller,
        service: &str,
        resource: &str,
    ) -> Option<Outcome> {
        if peers.mode() == Mode::Off || d.spec.get("RoleArn").is_some() {
            return None;
        }
        let (op, body, action, key) = if service == "sqs" {
            (
                "GetQueueAttributes",
                json!({"QueueUrl": resource, "AttributeNames": ["Policy"]}),
                "sqs:SendMessage",
                "Policy",
            )
        } else {
            (
                "GetTopicAttributes",
                json!({"TopicArn": resource}),
                "sns:Publish",
                "Policy",
            )
        };
        let policy = peers
            .call("events", caller, service, op, &body)
            .await
            .ok()
            .and_then(|v| v.get("Attributes")?.get(key)?.as_str().map(str::to_owned));
        let arn = if service == "sqs" {
            &d.target.arn
        } else {
            resource
        };
        let (_, account) = region_and_account(&d.rule_arn);
        if allows(policy.as_deref(), action, arn, &d.rule_arn, account) {
            return None;
        }
        let why = format!(
            "the {service} resource's policy does not let events.amazonaws.com deliver for {}",
            d.rule_arn
        );
        if peers.mode() == Mode::Enforce {
            return Some(Failure::fatal("AccessDenied", why));
        }
        tracing::warn!("iam soft mode: EventBridge delivery would be refused: {why}");
        None
    }
}
