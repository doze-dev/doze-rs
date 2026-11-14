//! One invocation of a target, made off the actor over the peer bus as the schedule's execution role (`role`): the role's session is
//! what the target service asks IAM about, so under `--iam enforce` a role that may not `sqs:SendMessage` fails the invocation, and
//! under `--iam soft` the refusal is logged.
//!
//! Templated targets call the one operation AWS documents for each: SQS `SendMessage` (`SqsParameters.MessageGroupId` for a FIFO
//! queue), SNS `Publish`, Lambda `Invoke` (asynchronous, `InvocationType: Event`), EventBridge `PutEvents` (`EventBridgeParameters`),
//! Step Functions `StartExecution` and Kinesis `PutRecord` (`KinesisParameters`). Universal targets are in `universal`. A target on a
//! service doze-aws does not run is not delivered, and the log says so; it is not dead-lettered (AWS would have delivered it).

use std::sync::Arc;

use aws_peers::Peers;
use aws_wire::ApiError;
use aws_wire::service::Caller;
use serde_json::{Value, json};

use crate::Shared;
use crate::fire::Invocation;
use crate::targets::{Kind, kind};

/// What came of one attempt.
#[derive(Debug)]
pub(crate) enum Outcome {
    Sent,
    /// Nothing here can deliver to the target.
    NotLocal(String),
    Failed(Failure),
}

/// Why an attempt failed, and whether trying again could help.
#[derive(Clone, Debug)]
pub(crate) struct Failure {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) retryable: bool,
}

impl Failure {
    pub(crate) fn of(e: &ApiError) -> Outcome {
        if e.message.starts_with("no service ") {
            return Outcome::NotLocal(e.message.clone());
        }
        // A server fault or throttling may pass; anything the target refused as wrong will not.
        let retryable = e.status >= 500 || e.code.contains("Throttl") || e.code.contains("Limit");
        Outcome::Failed(Self {
            code: e.code.to_string(),
            message: e.message.clone(),
            retryable,
        })
    }

    pub(crate) fn fatal(code: &str, message: impl Into<String>) -> Outcome {
        Outcome::Failed(Self {
            code: code.to_owned(),
            message: message.into(),
            retryable: false,
        })
    }
}

/// The invocation's `Input` with the context attributes put in (documented: `<aws.scheduler.schedule-arn>`,
/// `<aws.scheduler.scheduled-time>`, `<aws.scheduler.execution-id>`, `<aws.scheduler.attempt-number>`).
pub(crate) fn input(inv: &Invocation, attempt: u32) -> Option<String> {
    let text = inv.target.get("Input").and_then(Value::as_str)?;
    let time =
        aws_core::time::format_iso8601(inv.scheduled_ms.div_euclid(1000)).replace(".000Z", "Z");
    Some(
        text.replace("<aws.scheduler.schedule-arn>", &inv.schedule_arn)
            .replace("<aws.scheduler.scheduled-time>", &time)
            .replace("<aws.scheduler.execution-id>", &inv.execution_id)
            .replace("<aws.scheduler.attempt-number>", &attempt.to_string()),
    )
}

/// The caller an invocation is made as when it acts as the service itself (no role session).
pub(crate) fn base_caller(inv: &Invocation) -> Caller {
    Caller {
        access_key_id: "doze".to_owned(),
        account_id: inv.account.clone(),
        region: inv.region.clone(),
        endpoint: String::new(),
        service: None,
        session_token: None,
    }
}

/// Makes attempt number `attempt` of the invocation.
pub(crate) async fn attempt(sh: &Arc<Shared>, inv: &Invocation, attempt: u32) -> Outcome {
    let Some(peers) = sh.peers.clone() else {
        return Outcome::NotLocal("no other service is running".to_owned());
    };
    let role = inv
        .target
        .get("RoleArn")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let caller = match sh
        .sessions
        .caller(&peers, &base_caller(inv), role, sh.clock.now_ms())
        .await
    {
        Ok(c) => c,
        Err(refused) => return refused,
    };
    let arn = inv
        .target
        .get("Arn")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let payload = input(inv, attempt);
    let (outcome, service) = match kind(arn) {
        Some(Kind::Universal { sdk, action }) => (
            crate::universal::call(&peers, &caller, (sdk, action), payload.as_deref()).await,
            crate::targets::bus_name(sdk).unwrap_or_default(),
        ),
        Some(Kind::Templated { service }) => {
            let t = Templated {
                peers: &peers,
                caller: &caller,
                inv,
                arn,
                payload: payload.as_deref(),
            };
            (t.send(service).await, service)
        }
        None => return Failure::fatal("ValidationException", format!("Invalid target ARN {arn}.")),
    };
    match outcome {
        Outcome::Failed(mut f) => {
            f.code = client_code(&peers, service, &f.code);
            Outcome::Failed(f)
        }
        other => other,
    }
}

/// The code a client of the target service sees for an error: its `awsQueryError` code where the model gives one (SQS's
/// `QueueDoesNotExist` is `AWS.SimpleQueueService.NonExistentQueue`, as the documented dead-letter example shows), else its name.
fn client_code(peers: &Peers, service: &str, code: &str) -> String {
    let Some(t) = peers.tables_of(service) else {
        return code.to_owned();
    };
    (0..t.n_errs())
        .filter_map(|i| t.err(i))
        .find(|e| t.shape(e.shape).is_some_and(|s| s.name == code))
        .map_or_else(|| code.to_owned(), |e| e.code.to_owned())
}

/// A templated target's call.
struct Templated<'a> {
    peers: &'a Arc<Peers>,
    caller: &'a Caller,
    inv: &'a Invocation,
    arn: &'a str,
    payload: Option<&'a str>,
}

impl Templated<'_> {
    /// The payload, or `{}` when the target has no `Input` (memory: what AWS sends then is not recorded).
    fn body(&self) -> &str {
        self.payload.unwrap_or("{}")
    }

    fn param(&self, path: &str) -> Option<&Value> {
        self.inv.target.pointer(path)
    }

    async fn send(&self, service: &str) -> Outcome {
        let (op, body) = match service {
            "sqs" => return self.to_queue().await,
            "sns" => (
                "Publish",
                json!({"TopicArn": self.arn, "Message": self.body()}),
            ),
            "lambda" => (
                "Invoke",
                json!({"FunctionName": self.arn, "InvocationType": "Event",
                    "Payload": aws_core::base64::encode(self.body().as_bytes())}),
            ),
            "states" => (
                "StartExecution",
                json!({"stateMachineArn": self.arn, "input": self.body()}),
            ),
            "events" => return self.to_bus().await,
            "kinesis" => {
                let key = self
                    .param("/KinesisParameters/PartitionKey")
                    .and_then(Value::as_str)
                    .unwrap_or(&self.inv.execution_id);
                (
                    "PutRecord",
                    json!({"StreamARN": self.arn,
                    "Data": aws_core::base64::encode(self.body().as_bytes()), "PartitionKey": key}),
                )
            }
            other => {
                return Outcome::NotLocal(format!(
                    "targets on {other} are not delivered by doze-aws ({})",
                    self.arn
                ));
            }
        };
        let target = if service == "lambda" {
            "lambda"
        } else {
            service
        };
        match self.peers.call_as(self.caller, target, op, &body).await {
            Ok(_) => Outcome::Sent,
            Err(e) => Failure::of(&e),
        }
    }

    async fn to_queue(&self) -> Outcome {
        let url = match queue_url(self.peers, self.caller, self.arn).await {
            Ok(u) => u,
            Err(e) => return Failure::of(&e),
        };
        let mut body = json!({"QueueUrl": url, "MessageBody": self.body()});
        if let Some(group) = self.param("/SqsParameters/MessageGroupId") {
            put(&mut body, "MessageGroupId", group.clone());
            // (memory) a FIFO queue without content-based deduplication needs an id; the execution's is one per invocation.
            put(
                &mut body,
                "MessageDeduplicationId",
                json!(self.inv.execution_id),
            );
        }
        match self
            .peers
            .call_as(self.caller, "sqs", "SendMessage", &body)
            .await
        {
            Ok(_) => Outcome::Sent,
            Err(e) => Failure::of(&e),
        }
    }

    async fn to_bus(&self) -> Outcome {
        let entry = json!({
            "EventBusName": self.arn,
            "Source": self.param("/EventBridgeParameters/Source"),
            "DetailType": self.param("/EventBridgeParameters/DetailType"),
            "Detail": self.body(),
        });
        let body = json!({"Entries": [entry]});
        match self
            .peers
            .call_as(self.caller, "events", "PutEvents", &body)
            .await
        {
            Ok(v)
                if v.get("FailedEntryCount")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    > 0 =>
            {
                let e = v.pointer("/Entries/0").cloned().unwrap_or(Value::Null);
                let text = |k: &str| {
                    e.get(k)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                };
                Failure::fatal(&text("ErrorCode"), text("ErrorMessage"))
            }
            Ok(_) => Outcome::Sent,
            Err(e) => Failure::of(&e),
        }
    }
}

/// The URL of the queue an ARN names. Looked up as Scheduler itself: the role needs `sqs:SendMessage`, not `sqs:GetQueueUrl`.
pub(crate) async fn queue_url(
    peers: &Peers,
    caller: &Caller,
    arn: &str,
) -> Result<String, ApiError> {
    let mut parts = arn.split(':');
    let (region, account, name) = (parts.nth(3), parts.next(), parts.next());
    let as_queue = Caller {
        region: region.unwrap_or_default().to_owned(),
        account_id: account.unwrap_or_default().to_owned(),
        ..caller.clone()
    };
    let asked = json!({"QueueName": name.unwrap_or_default(), "QueueOwnerAWSAccountId": account});
    let got = peers
        .call("scheduler", &as_queue, "sqs", "GetQueueUrl", &asked)
        .await?;
    Ok(got
        .get("QueueUrl")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned())
}

/// Sets a member of a JSON object (a value that is not one is left as it is).
pub(crate) fn put(v: &mut Value, key: &str, value: Value) {
    if let Some(o) = v.as_object_mut() {
        o.insert(key.to_owned(), value);
    }
}
