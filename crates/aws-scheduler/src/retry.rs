//! An invocation from first attempt to its end: delivered; or retried under the target's `RetryPolicy` (`MaximumRetryAttempts`,
//! default 185; `MaximumEventAgeInSeconds`, default 86,400; both documented) and, when it gives up, sent to the target's
//! `DeadLetterConfig` queue with the attributes AWS documents (`ERROR_CODE`, `ERROR_MESSAGE`, `EXECUTION_ID`,
//! `EXHAUSTED_RETRY_CONDITION` for a retryable failure, `IS_PAYLOAD_TRUNCATED`, `RETRY_ATTEMPTS`, `SCHEDULED_TIME`, `SCHEDULE_ARN`,
//! `TARGET_ARN`). A failure the target refused as wrong is not retried.
//!
//! The wait between attempts doubles from one second to a minute (ours: AWS says only that it backs off). The waits run in a task off
//! the actor and a restart loses them, as EventBridge's do.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::Shared;
use crate::deliver::{Failure, Outcome, attempt, input, queue_url};
use crate::fire::Invocation;

const DEFAULT_ATTEMPTS: i64 = 185;
const DEFAULT_AGE_SECONDS: i64 = 86_400;

/// Seconds to wait after the `tries`-th failure.
pub(crate) fn backoff_seconds(tries: u32) -> u64 {
    1_u64 << tries.saturating_sub(1).min(6)
}

/// What to do after the `tries`-th failed attempt: wait and try again, or give up with an `EXHAUSTED_RETRY_CONDITION`.
pub(crate) fn next(
    inv: &Invocation,
    why: &Failure,
    tries: u32,
    now_ms: i64,
) -> Result<u64, Option<&'static str>> {
    let policy = |m: &str| {
        inv.target
            .pointer(&format!("/RetryPolicy/{m}"))
            .and_then(Value::as_i64)
    };
    let attempts = policy("MaximumRetryAttempts").unwrap_or(DEFAULT_ATTEMPTS);
    let age_ms = policy("MaximumEventAgeInSeconds").unwrap_or(DEFAULT_AGE_SECONDS) * 1000;
    if !why.retryable {
        return Err(None);
    }
    if now_ms - inv.scheduled_ms >= age_ms {
        return Err(Some("MaximumEventAgeInSeconds"));
    }
    if i64::from(tries.saturating_sub(1)) >= attempts {
        return Err(Some("MaximumRetryAttempts"));
    }
    Ok(backoff_seconds(tries))
}

/// Carries an invocation on after attempt `tries` came to `outcome`: waits and tries again, or dead-letters it.
pub(crate) async fn carry_on(
    sh: Arc<Shared>,
    inv: Invocation,
    mut tries: u32,
    mut outcome: Outcome,
) {
    let stopping = sh.tasks.stopping();
    loop {
        let why = match outcome {
            Outcome::Sent => return,
            Outcome::NotLocal(why) => {
                tracing::warn!(schedule = %inv.schedule_arn, "EventBridge Scheduler did not deliver: {why}");
                return;
            }
            Outcome::Failed(why) => why,
        };
        match next(&inv, &why, tries, sh.clock.now_ms()) {
            Ok(wait) => {
                tracing::debug!(schedule = %inv.schedule_arn, "invocation failed, will retry: {}", why.message);
                tokio::select! {
                    () = tokio::time::sleep(Duration::from_secs(wait)) => {}
                    () = stopping.cancelled() => return,
                }
            }
            Err(condition) => {
                dead_letter(&sh, &inv, &why, tries, condition).await;
                return;
            }
        }
        tries += 1;
        outcome = attempt(&sh, &inv, tries).await;
    }
}

/// Sends the invocation to the target's dead-letter queue as the execution role, if it has one; the log says so either way.
async fn dead_letter(
    sh: &Arc<Shared>,
    inv: &Invocation,
    why: &Failure,
    tries: u32,
    condition: Option<&str>,
) {
    let queue = inv
        .target
        .pointer("/DeadLetterConfig/Arn")
        .and_then(Value::as_str);
    let (Some(queue), Some(peers)) = (queue, sh.peers.clone()) else {
        tracing::warn!(schedule = %inv.schedule_arn, code = %why.code, "EventBridge Scheduler could not invoke the target, and it has no dead-letter queue: {}", why.message);
        return;
    };
    let base = crate::deliver::base_caller(inv);
    let role = inv
        .target
        .get("RoleArn")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Ok(caller) = sh
        .sessions
        .caller(&peers, &base, role, sh.clock.now_ms())
        .await
    else {
        tracing::warn!(schedule = %inv.schedule_arn, "EventBridge Scheduler could not assume the role to dead-letter: {}", why.message);
        return;
    };
    let text = |v: &str| json!({"DataType": "String", "StringValue": v});
    let time =
        aws_core::time::format_iso8601(inv.scheduled_ms.div_euclid(1000)).replace(".000Z", "Z");
    let mut attrs = json!({
        "ERROR_CODE": text(&why.code),
        "ERROR_MESSAGE": text(&why.message),
        "EXECUTION_ID": text(&inv.execution_id),
        "IS_PAYLOAD_TRUNCATED": text("false"),
        "RETRY_ATTEMPTS": text(&tries.saturating_sub(1).to_string()),
        "SCHEDULED_TIME": text(&time),
        "SCHEDULE_ARN": text(&inv.schedule_arn),
        "TARGET_ARN": text(inv.target.get("Arn").and_then(Value::as_str).unwrap_or_default()),
    });
    if let Some(c) = condition {
        crate::page::put(&mut attrs, "EXHAUSTED_RETRY_CONDITION", text(c));
    }
    let sent = async {
        let url = queue_url(&peers, &caller, queue).await?;
        let body = json!({
            "QueueUrl": url,
            "MessageBody": input(inv, tries).unwrap_or_else(|| "{}".to_owned()),
            "MessageAttributes": attrs,
        });
        peers.call_as(&caller, "sqs", "SendMessage", &body).await
    };
    if let Err(e) = sent.await {
        tracing::warn!(schedule = %inv.schedule_arn, "EventBridge Scheduler could not reach the dead-letter queue {queue}: {}", e.message);
    }
}
