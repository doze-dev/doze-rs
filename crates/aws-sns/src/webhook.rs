//! `http` and `https` subscriptions: the endpoint is sent a `SubscriptionConfirmation` when it subscribes, and once it has confirmed (by fetching
//! the `SubscribeURL`, or calling `ConfirmSubscription` with the token) every notification is POSTed to it, as AWS sends them: the
//! headers `x-amz-sns-message-type`, `x-amz-sns-message-id`, `x-amz-sns-topic-arn` and (for a notification)
//! `x-amz-sns-subscription-arn`, `Content-Type: text/plain; charset=UTF-8`, and the JSON document of the SNS Developer Guide's "HTTP/HTTPS
//! … JSON format" pages as the body (the raw message, with `x-amz-sns-rawdelivery: true`, for a raw subscription). The confirmation's
//! `Message` sentence is the documented one; the `User-Agent` is (memory).
//!
//! **Off the actor.** Each POST is a task: the publish answers without waiting for the endpoint. A delivery that fails (no answer, a
//! 429 or a 5xx; AWS does not retry other client errors (memory)) is tried again under the delivery policy (the subscription's
//! `DeliveryPolicy`, else the topic's effective one: three retries 20 s apart by default), and then dropped with a log line.
//!
//! **`https` endpoints** are called over TLS (`aws-webhook`): the certificate must be one the machine trusts, as AWS requires a
//! certificate from a trusted authority (memory). A local server with a self-signed certificate is refused, and the log names the
//! flag that fixes it (`--ca-file` or `--insecure-outbound-tls`). An attempt waits at most 15 seconds for the endpoint (memory: the
//! SNS guides do not give the figure).
//!
//! The `SubscribeURL` is `<endpoint>/_aws/sns/confirm?Action=ConfirmSubscription&TopicArn=…&Token=…`: AWS's query on a doze path (an
//! unsigned request to `/` is S3's), answered by [`crate::Sns`] as a web address.

use std::sync::Arc;
use std::time::Duration;

use aws_webhook::{Failure, Request};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::core::Core;
use crate::deliver::Published;
use crate::envelope::{self, Context};
use crate::model::Subscription;

/// The path the `SubscribeURL` names.
pub(crate) const CONFIRM_PATH: &str = "/_aws/sns/confirm";
/// How long one attempt waits for the endpoint (memory).
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(15);
/// The most POSTs waiting or under way; past it a notification is dropped with a log line.
const MAX_UNDER_WAY: usize = 10_000;

/// One POST to make, and again under a delivery policy.
struct Post {
    url: String,
    headers: Vec<(String, String)>,
    body: String,
    /// The waits between attempts: one per retry.
    retries: Vec<u64>,
    what: String,
    stop: CancellationToken,
    under_way: Arc<std::sync::atomic::AtomicUsize>,
}

/// The waits, in seconds, between the attempts the delivery policy allows (`healthyRetryPolicy` or the topic's
/// `http.defaultHealthyRetryPolicy`): the no-delay retries, the retries at the minimum, the backoff phase (linear from the minimum to
/// the maximum; AWS's other backoff functions are approximated by it (memory)), and the retries at the maximum.
pub(crate) fn retry_waits(policy: &Value) -> Vec<u64> {
    let p = policy
        .get("healthyRetryPolicy")
        .or_else(|| policy.pointer("/http/defaultHealthyRetryPolicy"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let n = |k: &str, d: u64| p.get(k).and_then(Value::as_u64).unwrap_or(d);
    let (min, max) = (n("minDelayTarget", 20), n("maxDelayTarget", 20).max(1));
    let total = n("numRetries", 3).min(100);
    let (none, at_min, at_max) = (
        n("numNoDelayRetries", 0),
        n("numMinDelayRetries", 0),
        n("numMaxDelayRetries", 0),
    );
    let backoff = total.saturating_sub(none + at_min + at_max);
    let mut out = Vec::new();
    for i in 0..total {
        let wait = if i < none {
            0
        } else if i < none + at_min {
            min
        } else if i < none + at_min + backoff {
            let step = i - none - at_min + 1;
            min + (max.saturating_sub(min)) * step / backoff.max(1)
        } else {
            max
        };
        out.push(wait.min(max.max(min)));
    }
    out
}

impl Core {
    /// Sends a new `http` or `https` subscription its `SubscriptionConfirmation`.
    pub(crate) fn ask_to_confirm(&self, sub: &Subscription, endpoint: &str) {
        let Some(token) = sub.token.as_deref() else {
            return;
        };
        let message_id = aws_seal::uuid_v4().unwrap_or_default();
        let mut doc = serde_json::Map::new();
        doc.insert("Type".into(), json!("SubscriptionConfirmation"));
        doc.insert("MessageId".into(), json!(message_id));
        doc.insert("Token".into(), json!(token));
        doc.insert("TopicArn".into(), json!(sub.topic_arn));
        doc.insert("Message".into(), json!(format!("You have chosen to subscribe to the topic {}.\nTo confirm the subscription, visit the SubscribeURL included in this message.", sub.topic_arn)));
        // A peer call has no address worth naming; the token alone still confirms through ConfirmSubscription.
        if endpoint.starts_with("http") {
            let enc = |s: &str| aws_core::uri::encode(s.as_bytes(), false);
            doc.insert(
                "SubscribeURL".into(),
                json!(format!(
                    "{endpoint}{CONFIRM_PATH}?Action=ConfirmSubscription&TopicArn={}&Token={}",
                    enc(&sub.topic_arn),
                    enc(token)
                )),
            );
        }
        doc.insert(
            "Timestamp".into(),
            json!(crate::publish::timestamp(self.now_ms())),
        );
        doc.insert("SignatureVersion".into(), json!("1"));
        doc.insert("Signature".into(), json!(""));
        doc.insert(
            "SigningCertURL".into(),
            json!(format!("{endpoint}/SimpleNotificationService-doze.pem")),
        );
        let headers = vec![
            (
                "x-amz-sns-message-type".to_owned(),
                "SubscriptionConfirmation".to_owned(),
            ),
            ("x-amz-sns-message-id".to_owned(), message_id),
            ("x-amz-sns-topic-arn".to_owned(), sub.topic_arn.clone()),
        ];
        self.post(
            sub,
            headers,
            Value::Object(doc).to_string(),
            Vec::new(),
            "confirmation",
        );
    }

    /// POSTs one notification to a confirmed `http` or `https` subscription.
    pub(crate) fn to_endpoint(&self, p: &Published<'_>, sub: &Subscription) -> Result<(), String> {
        let text = p.message.text_for("http");
        let mut headers = vec![
            (
                "x-amz-sns-message-type".to_owned(),
                "Notification".to_owned(),
            ),
            ("x-amz-sns-message-id".to_owned(), p.message_id.to_owned()),
            ("x-amz-sns-topic-arn".to_owned(), p.topic.arn.clone()),
            ("x-amz-sns-subscription-arn".to_owned(), sub.arn.clone()),
        ];
        let body = if sub.raw() {
            headers.push(("x-amz-sns-rawdelivery".to_owned(), "true".to_owned()));
            text
        } else {
            let ctx = Context {
                message_id: p.message_id,
                topic_arn: &p.topic.arn,
                subscription_arn: &sub.arn,
                timestamp: p.timestamp,
                endpoint: &p.caller.endpoint,
            };
            envelope::notification(&ctx, p.message, &text)
        };
        let policy = sub
            .attrs
            .get("DeliveryPolicy")
            .cloned()
            .or_else(|| {
                self.topics
                    .get(&p.topic.name)
                    .map(crate::model::Topic::effective_delivery)
            })
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .unwrap_or(Value::Null);
        self.post(sub, headers, body, retry_waits(&policy), "notification");
        Ok(())
    }

    fn post(
        &self,
        sub: &Subscription,
        mut headers: Vec<(String, String)>,
        body: String,
        retries: Vec<u64>,
        what: &str,
    ) {
        use std::sync::atomic::Ordering;
        if self.webhooks.load(Ordering::Acquire) >= MAX_UNDER_WAY {
            tracing::warn!(subscription = %sub.arn, "SNS has too many http deliveries under way; this {what} was dropped");
            return;
        }
        headers.push((
            "Content-Type".to_owned(),
            "text/plain; charset=UTF-8".to_owned(),
        ));
        // (memory)
        headers.push((
            "User-Agent".to_owned(),
            "Amazon Simple Notification Service Agent".to_owned(),
        ));
        self.webhooks.fetch_add(1, Ordering::AcqRel);
        self.tasks.spawn(send(Post {
            url: sub.endpoint.clone(),
            headers,
            body,
            retries,
            what: format!("{what} for {}", sub.arn),
            stop: self.tasks.stopping(),
            under_way: Arc::clone(&self.webhooks),
        }));
    }
}

async fn send(post: Post) {
    let mut waits = post.retries.iter();
    loop {
        let sent = aws_webhook::fetch(&Request {
            headers: &post.headers,
            body: post.body.as_bytes(),
            timeout: ATTEMPT_TIMEOUT,
            status_only: true,
            ..Request::new("POST", &post.url)
        })
        .await;
        let why = match sent {
            Ok(a) if (200..300).contains(&a.status) => break,
            Ok(a) if a.status == 429 || a.status >= 500 => {
                format!("{} answered {}", post.url, a.status)
            }
            Ok(a) => {
                tracing::warn!(
                    "SNS {}: {} answered {}; not retried",
                    post.what,
                    post.url,
                    a.status
                );
                break;
            }
            Err(Failure::Unsupported(why)) => {
                tracing::warn!("SNS {}: {why}", post.what);
                break;
            }
            // A refused certificate is said at once, with its fix: it will not mend itself between retries.
            Err(Failure::Tls(why)) => {
                tracing::warn!("SNS {}: {why}", post.what);
                why
            }
            Err(other) => other.to_string(),
        };
        let Some(wait) = waits.next() else {
            tracing::warn!(
                "SNS {}: {why}; the delivery policy's retries are spent",
                post.what
            );
            break;
        };
        tracing::debug!("SNS {}: {why}; retrying in {wait} s", post.what);
        tokio::select! {
            () = tokio::time::sleep(Duration::from_secs(*wait)) => {}
            () = post.stop.cancelled() => break,
        }
    }
    post.under_way
        .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_policy_is_three_retries_twenty_seconds_apart() {
        let default = json!({"http": {"defaultHealthyRetryPolicy": {"minDelayTarget": 20, "maxDelayTarget": 20, "numRetries": 3}}});
        assert_eq!(retry_waits(&default), [20, 20, 20]);
        assert_eq!(retry_waits(&Value::Null), [20, 20, 20]);
        let phased = json!({"healthyRetryPolicy": {"minDelayTarget": 1, "maxDelayTarget": 9, "numRetries": 6,
            "numNoDelayRetries": 1, "numMinDelayRetries": 1, "numMaxDelayRetries": 1}});
        assert_eq!(retry_waits(&phased), [0, 1, 3, 6, 9, 9]);
        let none = json!({"healthyRetryPolicy": {"numRetries": 0}});
        assert!(retry_waits(&none).is_empty());
    }
}
