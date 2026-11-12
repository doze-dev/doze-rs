//! Asynchronous invocation (`InvocationType: Event`): accepted at once, run in a tracked task, retried when it fails, and the result
//! sent to the destinations the function's invoke config names (SQS, SNS, another function, an event bus) or to its dead-letter
//! queue. The delay between retries is a second and two seconds (AWS waits a minute and two): a developer testing a failure path
//! does not wait minutes.

use std::sync::Arc;
use std::time::Duration;

use aws_core::time::format_iso8601;
use aws_wire::service::Caller;
use bytes::Bytes;
use serde_json::{Value, json};

use crate::Inner;
use crate::invoke::{Input, RunOut};
use crate::resolve::Resolved;

/// AWS's defaults: two retries, six hours.
const DEFAULT_RETRIES: i64 = 2;
const DEFAULT_AGE_SECS: i64 = 21_600;

/// What a destination is told: the invocation, its input, and how it ended.
struct Result<'a> {
    rs: &'a Resolved,
    out: Option<&'a RunOut>,
    payload: &'a Bytes,
    condition: &'static str,
    attempts: i64,
    why: Option<String>,
}

impl Inner {
    /// Runs the function in the background.
    pub(crate) fn async_invocation(self: &Arc<Self>, rs: Box<Resolved>, caller: Caller, payload: Bytes, context: Option<String>) {
        let me = Arc::clone(self);
        self.tasks.spawn(async move { me.run_async(*rs, caller, payload, context).await });
    }

    async fn run_async(self: &Arc<Self>, rs: Resolved, caller: Caller, payload: Bytes, context: Option<String>) {
        let settings = rs.event_invoke.clone().unwrap_or(Value::Null);
        let retries = settings.get("MaximumRetryAttempts").and_then(Value::as_i64).unwrap_or(DEFAULT_RETRIES);
        let max_age = settings.get("MaximumEventAgeInSeconds").and_then(Value::as_i64).unwrap_or(DEFAULT_AGE_SECS);
        let began = self.clock.now_ms();
        let stop = self.tasks.stopping();
        let mut attempts = 0_i64;
        loop {
            attempts += 1;
            let input = Input { payload: payload.clone(), client_context: context.clone(), trace_id: String::new() };
            let ran = self.run_function(&rs, &caller, input).await;
            let (out, why) = match ran {
                Ok(out) if !out.error => {
                    self.deliver(&settings, &caller, &Result { rs: &rs, out: Some(&out), payload: &payload, condition: "Success", attempts, why: None }).await;
                    return;
                }
                Ok(out) => (Some(out), None),
                Err(e) => (None, Some(e.message)),
            };
            let aged = (self.clock.now_ms() - began) / 1000 >= max_age;
            if attempts > retries || aged {
                let condition = if aged { "EventAgeExceeded" } else { "RetriesExhausted" };
                let res = Result { rs: &rs, out: out.as_ref(), payload: &payload, condition, attempts, why };
                self.deliver(&settings, &caller, &res).await;
                self.dead_letter(&caller, &res).await;
                return;
            }
            let wait = Duration::from_secs(u64::try_from(attempts).unwrap_or(1));
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                () = stop.cancelled() => return,
            }
        }
    }

    /// The document a destination receives.
    fn destination_document(&self, r: &Result<'_>) -> Value {
        let parse = |b: &[u8]| serde_json::from_slice::<Value>(b).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(b).into_owned()));
        let mut response = json!({"statusCode": 200, "executedVersion": r.rs.version});
        let body = match r.out {
            Some(out) => {
                if out.error {
                    response["functionError"] = json!("Unhandled");
                }
                parse(&out.payload)
            }
            None => json!({"errorMessage": r.why}),
        };
        json!({
            "version": "1.0",
            "timestamp": format!("{}Z", format_iso8601(self.clock.now_ms() / 1000).trim_end_matches('Z')),
            "requestContext": {
                "requestId": r.out.map(|o| o.request_id.clone()),
                "functionArn": r.rs.invoked_arn,
                "condition": r.condition,
                "approximateInvokeCount": r.attempts,
            },
            "requestPayload": parse(r.payload),
            "responseContext": response,
            "responsePayload": body,
        })
    }

    async fn deliver(self: &Arc<Self>, settings: &Value, caller: &Caller, r: &Result<'_>) {
        let which = if r.condition == "Success" { "OnSuccess" } else { "OnFailure" };
        let Some(target) = settings.pointer(&format!("/DestinationConfig/{which}/Destination")).and_then(Value::as_str) else { return };
        let doc = self.destination_document(r).to_string();
        if let Err(why) = self.send_to(caller, target, &doc, r.rs).await {
            tracing::warn!(function = %r.rs.name, "could not deliver the result to {target}: {why}");
        }
    }

    /// The dead-letter queue or topic of a function whose asynchronous invocation failed for good.
    async fn dead_letter(self: &Arc<Self>, caller: &Caller, r: &Result<'_>) {
        let Some(target) = r.rs.config.get("DeadLetterConfig").and_then(|d| d.get("TargetArn")).and_then(Value::as_str) else { return };
        let body = String::from_utf8_lossy(r.payload).into_owned();
        if let Err(why) = self.send_to(caller, target, &body, r.rs).await {
            tracing::warn!(function = %r.rs.name, "could not send the event to its dead-letter target {target}: {why}");
        }
    }

    /// Sends `message` to an SQS queue, an SNS topic, another function or an event bus, as Lambda.
    pub(crate) async fn send_to(self: &Arc<Self>, caller: &Caller, target: &str, message: &str, rs: &Resolved) -> std::result::Result<(), String> {
        let peers = self.peers.as_deref().ok_or("no other service is running")?;
        let (region, account) = crate::arn::region_account(target).ok_or("the destination is not an ARN")?;
        let acting = Caller { region: region.to_owned(), account_id: account.to_owned(), service: Some("lambda".to_owned()), ..caller.clone() };
        let service = target.split(':').nth(2).unwrap_or_default();
        let call = |svc: &'static str, op: &'static str, body: Value| {
            let acting = acting.clone();
            async move { peers.call("lambda", &acting, svc, op, &body).await.map_err(|e| e.message) }
        };
        match service {
            "sqs" => {
                let name = target.rsplit(':').next().unwrap_or_default();
                let url = call("sqs", "GetQueueUrl", json!({"QueueName": name, "QueueOwnerAWSAccountId": account})).await?;
                let url = url.get("QueueUrl").and_then(Value::as_str).ok_or("no queue url")?.to_owned();
                let mut body = json!({"QueueUrl": url, "MessageBody": message});
                if name.ends_with(".fifo") {
                    body["MessageGroupId"] = json!(rs.name);
                    body["MessageDeduplicationId"] = json!(aws_seal::uuid_v4().unwrap_or_default());
                }
                call("sqs", "SendMessage", body).await.map(|_| ())
            }
            "sns" => call("sns", "Publish", json!({"TopicArn": target, "Message": message})).await.map(|_| ()),
            "events" => {
                let entry = json!({"EventBusName": target, "Source": "lambda", "DetailType": "Lambda Function Invocation Result", "Detail": message});
                call("events", "PutEvents", json!({"Entries": [entry]})).await.map(|_| ())
            }
            "lambda" => self.invoke_internal(caller, target, message).await,
            other => Err(format!("a {other} destination is not delivered locally")),
        }
    }

    /// Invokes the function `target` names with `message`, in the background, as the service does for a destination.
    async fn invoke_internal(self: &Arc<Self>, caller: &Caller, target: &str, message: &str) -> std::result::Result<(), String> {
        let ask = crate::resolve::Resolving {
            caller: Caller { service: Some("lambda".to_owned()), ..caller.clone() },
            function: target.to_owned(),
            qualifier: None,
            source_arn: None,
            draw: 0.0,
            url: false,
            internal: true,
        };
        let region = crate::arn::region_account(target).map_or(caller.region.as_str(), |(r, _)| r).to_owned();
        match self.ask(&region, crate::core::Ask::Resolve(Box::new(ask))).await.map_err(|e| e.message)? {
            crate::core::Answer::Resolved(Ok(rs)) => {
                self.async_invocation(rs, caller.clone(), Bytes::from(message.to_owned()), None);
                Ok(())
            }
            crate::core::Answer::Resolved(Err(e)) => Err(e.message),
            _ => Err("unexpected answer".to_owned()),
        }
    }
}
