//! A delivery that failed: tried again with a backoff, and when it is out of tries or too old, sent to the target's dead-letter queue
//! or dropped.
//!
//! The retry policy is the target's `RetryPolicy`: `MaximumRetryAttempts` (default 185) and `MaximumEventAgeInSeconds` (default
//! 86,400), both documented. A failure that cannot pass (the target refused it as wrong: a queue that is not there, a policy that does
//! not allow it) is not retried. The backoff, doubling from one second to a minute, is ours; AWS says only "exponential backoff with
//! jitter" (memory). The retries wait in memory and a restart loses them (SCOPE.md: the 24-hour retry is not needed, the difference is
//! flagged).
//!
//! The dead-letter message is the original event with the attributes AWS documents: `RULE_ARN`, `TARGET_ARN`, `ERROR_CODE`,
//! `ERROR_MESSAGE`, `EXHAUSTED_RETRY_CONDITION` and `RETRY_ATTEMPTS`.

use aws_wire::service::Caller;
use serde_json::{Value, json};

use crate::core::Core;
use crate::deliver::{Delivery, Failure};
use crate::names::region_and_account;

const DEFAULT_ATTEMPTS: i64 = 185;
const DEFAULT_AGE_SECONDS: i64 = 86_400;
/// The most deliveries waiting for another try; past it a failure goes straight to the dead-letter queue.
const MAX_WAITING: usize = 10_000;

/// A failed delivery waiting for its next try.
#[derive(Debug)]
pub(crate) struct Retry {
    pub(crate) delivery: Delivery,
    /// How many times it has been tried.
    pub(crate) tries: u32,
    pub(crate) due_ms: i64,
    pub(crate) caller: Caller,
}

/// Seconds to wait after the `tries`-th failure.
fn backoff_seconds(tries: u32) -> i64 {
    1_i64 << tries.saturating_sub(1).min(6)
}

impl Core {
    /// A delivery has just failed for the `tries`-th time: wait for another try, or give up.
    pub(crate) async fn failed(&mut self, d: Delivery, why: Failure, tries: u32, caller: &Caller) {
        let policy = |m: &str| {
            d.spec
                .pointer(&format!("/RetryPolicy/{m}"))
                .and_then(Value::as_i64)
        };
        let attempts = policy("MaximumRetryAttempts").unwrap_or(DEFAULT_ATTEMPTS);
        let age_ms = policy("MaximumEventAgeInSeconds").unwrap_or(DEFAULT_AGE_SECONDS) * 1000;
        let now = self.now_ms();
        let spent = i64::from(tries.saturating_sub(1)) >= attempts;
        let too_old = now - d.event.time_ms >= age_ms;
        if why.retryable && !spent && !too_old && self.retries.len() < MAX_WAITING {
            tracing::debug!(rule = %d.rule_arn, target = %d.target.id, "delivery failed, will retry: {}", why.message);
            self.retries.push(Retry {
                due_ms: now + backoff_seconds(tries) * 1000,
                delivery: d,
                tries,
                caller: caller.clone(),
            });
            return;
        }
        let condition = if why.retryable && too_old {
            "MaximumEventAge"
        } else {
            "MaximumRetryAttempts"
        };
        self.dead_letter(&d, &why, tries, condition, caller).await;
    }

    /// Sends the event to the target's dead-letter queue, if it has one; says so in the log either way.
    async fn dead_letter(
        &self,
        d: &Delivery,
        why: &Failure,
        tries: u32,
        condition: &str,
        caller: &Caller,
    ) {
        let queue = d
            .spec
            .pointer("/DeadLetterConfig/Arn")
            .and_then(Value::as_str)
            .filter(|a| a.contains(":sqs:"));
        let Some(queue) = queue else {
            tracing::warn!(rule = %d.rule_arn, target = %d.target.id, code = %why.code, "EventBridge could not deliver, and the target has no dead-letter queue: {}", why.message);
            return;
        };
        let Some(peers) = self.peers.clone() else {
            return;
        };
        let (region, account) = region_and_account(queue);
        let mut as_queue = caller.clone();
        as_queue.region = region.to_owned();
        as_queue.account_id = account.to_owned();
        let name = queue.rsplit(':').next().unwrap_or_default();
        let text = |v: &str| json!({"DataType": "String", "StringValue": v});
        let sent = async {
            let url = peers
                .call(
                    "events",
                    &as_queue,
                    "sqs",
                    "GetQueueUrl",
                    &json!({"QueueName": name, "QueueOwnerAWSAccountId": account}),
                )
                .await?;
            let body = json!({
                "QueueUrl": url.get("QueueUrl"),
                "MessageBody": d.event.text(),
                "MessageAttributes": {
                    "RULE_ARN": text(&d.rule_arn),
                    "TARGET_ARN": text(&d.target.arn),
                    "ERROR_CODE": text(&why.code),
                    "ERROR_MESSAGE": text(&why.message),
                    "EXHAUSTED_RETRY_CONDITION": text(condition),
                    "RETRY_ATTEMPTS": json!({"DataType": "Number", "StringValue": tries.saturating_sub(1).to_string()}),
                },
            });
            peers
                .call("events", &as_queue, "sqs", "SendMessage", &body)
                .await
        };
        if let Err(e) = sent.await {
            tracing::warn!(rule = %d.rule_arn, "EventBridge could not reach the dead-letter queue {queue}: {}", e.message);
        }
    }

    /// The earliest time a waiting retry is due.
    pub(crate) fn next_retry(&self) -> Option<i64> {
        self.retries.iter().map(|r| r.due_ms).min()
    }
}
