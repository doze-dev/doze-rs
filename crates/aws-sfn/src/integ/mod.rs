//! Task integrations, run outside the actor: the call is made over the peer bus as the machine's role (an STS session IAM
//! resolves), its answer read back as the result, and the outcome handed to the actor. A Task never ends in a Rust error: every
//! outcome is a result or a failure `Retry` and `Catch` can match (`Lambda.ResourceNotFoundException`, `DynamoDB.
//! ConditionalCheckFailedException`, the function's own `errorType`, ...).
//!
//! An integration whose service does not run here (ECS, Glue, Batch, a `.sync` on anything but Step Functions) fails the state
//! with `States.TaskFailed` and a cause that says so (decision D3): a `Catch` can take it, and nothing is faked.

mod http;
mod lambda;
mod optimized;
mod reader;
mod sdk;

use std::sync::Arc;

use aws_asl::{Failure, Outcome};
use aws_peers::Peers;
use aws_wire::ApiError;
use aws_wire::service::Caller;
use serde_json::{Value, json};

use crate::core::Request;
use crate::effects::Job;
use crate::shared::Shared;

/// Makes the call of `job` and hands the outcome to the actor.
pub(crate) async fn run(shared: Arc<Shared>, starter: Caller, job: Job) {
    let outcome = outcome(&shared, &starter, &job).await;
    let caller = Caller {
        region: job.region.clone(),
        account_id: job.account.clone(),
        ..starter
    };
    let req = Request::Result {
        execution: job.execution.clone(),
        frame: job.frame,
        seq: job.seq,
        outcome,
    };
    // WHY: an execution whose region cannot be opened any more has nothing to be told.
    drop(shared.ask(&job.region, &caller, req).await);
}

async fn outcome(shared: &Shared, starter: &Caller, job: &Job) -> Outcome {
    let Some(peers) = shared.peers.clone() else {
        return not_local(&job.resource, "no other service runs in this process");
    };
    let base = Caller {
        access_key_id: starter.access_key_id.clone(),
        account_id: job.account.clone(),
        region: job.region.clone(),
        endpoint: if job.endpoint.is_empty() {
            starter.endpoint.clone()
        } else {
            job.endpoint.clone()
        },
        service: None,
        session_token: None,
    };
    let now = shared.ctx.clock.now_ms();
    let role = job
        .credentials
        .as_ref()
        .and_then(|c| c.get("RoleArn"))
        .and_then(Value::as_str)
        .unwrap_or(&job.role_arn);
    let caller = shared.sessions.caller(&peers, &base, role, now).await;
    let got = call(&peers, &caller, job).await;
    match (got, &job.token) {
        (Ok(v), Some(_)) => Outcome::Submitted(v),
        (Ok(v), None) => Outcome::Done(v),
        (Err(f), _) => Outcome::Failed(f),
    }
}

/// The call a resource names.
pub(crate) async fn call(peers: &Arc<Peers>, caller: &Caller, job: &Job) -> Result<Value, Failure> {
    let r = job.resource.as_str();
    if r == aws_asl::ITEM_READER {
        return reader::read(peers, caller, &job.input).await;
    }
    if r.starts_with("arn:aws:lambda:") {
        return lambda::bare(peers, caller, r, &job.input).await;
    }
    let Some(rest) = r.strip_prefix("arn:aws:states:::") else {
        return Err(not_local_failure(
            r,
            "it is not a resource Step Functions can call",
        ));
    };
    let (base, pattern) = match rest.split_once('.') {
        Some((b, p)) => (b, p),
        None => (rest, ""),
    };
    if !pattern.is_empty() && pattern != "waitForTaskToken" {
        return Err(not_local_failure(
            r,
            "only Step Functions' own .sync runs locally: there is no job here to wait for",
        ));
    }
    match base {
        "lambda:invoke" => lambda::optimized(peers, caller, &job.input).await,
        "sqs:sendMessage" => optimized::sqs(peers, caller, &job.input).await,
        "sns:publish" => optimized::sns(peers, caller, &job.input).await,
        "events:putEvents" => optimized::events(peers, caller, &job.input).await,
        "dynamodb:getItem" | "dynamodb:putItem" | "dynamodb:updateItem" | "dynamodb:deleteItem" => {
            optimized::dynamodb(
                peers,
                caller,
                base.trim_start_matches("dynamodb:"),
                &job.input,
            )
            .await
        }
        "http:invoke" => http::invoke(peers, caller, &job.input).await,
        other => match other.strip_prefix("aws-sdk:") {
            Some(sdk) => sdk::call(peers, caller, r, sdk, &job.input).await,
            None => Err(not_local_failure(r, "doze-aws has no such integration")),
        },
    }
}

/// `States.TaskFailed` for a resource nothing here can call.
pub(crate) fn not_local_failure(resource: &str, why: &str) -> Failure {
    Failure::new(
        aws_asl::errors::TASK_FAILED,
        format!("doze-aws cannot call {resource} locally: {why}"),
    )
}

fn not_local(resource: &str, why: &str) -> Outcome {
    Outcome::Failed(not_local_failure(resource, why))
}

/// The failure a service's error becomes: `<Prefix>.<Code>`, with `Exception` added as AWS's SDK names it (memory), or the
/// not-local failure when the service is not running.
pub(crate) fn failure(prefix: &str, resource: &str, e: &ApiError) -> Failure {
    if e.message.starts_with("no service ") {
        return not_local_failure(resource, &e.message);
    }
    if e.code == "AccessDeniedException" || e.code == "AccessDenied" {
        return Failure::new(format!("{prefix}.AccessDeniedException"), e.message.clone());
    }
    let code = if e.code.ends_with("Exception") {
        e.code.to_string()
    } else {
        format!("{}Exception", e.code)
    };
    Failure::new(format!("{prefix}.{code}"), e.message.clone())
}

/// What AWS's optimized integrations add to a service's answer (memory: the members, not the values).
pub(crate) fn with_metadata(mut v: Value, status: u16) -> Value {
    if let Some(m) = v.as_object_mut() {
        let id = crate::util::uuid();
        m.insert(
            "SdkHttpMetadata".into(),
            json!({"AllHttpHeaders": {"x-amzn-RequestId": [id]}, "HttpHeaders": {"x-amzn-RequestId": id}, "HttpStatusCode": status}),
        );
        m.insert("SdkResponseMetadata".into(), json!({"RequestId": id}));
    }
    v
}

/// A JSON value as the text a string member wants: a string as it is, anything else as JSON.
pub(crate) fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
