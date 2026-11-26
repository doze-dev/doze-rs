//! Forced events (the console's Lab): a schedule is invoked now.
//!
//! It is the invocation the schedule's timer makes: the same record handed out (`Invocation`), the same first attempt over the bus as
//! the execution role (`deliver::attempt`), and the same retry policy and dead-letter queue after it (`retry::carry_on`). A recurring
//! schedule keeps its plan, so its next fire still happens when it was due; a one-time schedule's fire is its one fire. What the timer
//! would not fire at all (a disabled schedule, one with no fire left) is refused in words, and nothing is invoked.
//!
//! The authorizer is asked for `scheduler:UpdateSchedule` on the schedule: whoever may change when it fires may make it fire.

use std::time::Duration;

use aws_peers::{Mode, Need};
use aws_wire::ApiError;
use aws_wire::forced::{self, Done, Forced, Step, kind};
use aws_wire::service::{BoxFuture, Caller};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::Scheduler;
use crate::core::Request;
use crate::deliver::{Outcome, attempt};
use crate::fire::{Invocation, NOTHING};
use crate::model::{DEFAULT_GROUP, schedule_arn};
use crate::targets::{Kind, kind as target_kind};

/// How long the Lab waits for the first attempt before it says the invocation carries on without it.
const FIRST_ATTEMPT: Duration = Duration::from_secs(10);

impl Forced for Scheduler {
    fn force<'a>(
        &'a self,
        caller: &'a Caller,
        what: &'a str,
        input: &'a Value,
    ) -> BoxFuture<'a, Result<Step, ApiError>> {
        Box::pin(async move {
            if what != kind::FIRE_SCHEDULE {
                return Err(ApiError::internal(format!(
                    "Scheduler has no forced event {what}"
                )));
            }
            let name = forced::text(input, "name").ok_or_else(|| forced::missing("a schedule"))?;
            let group = forced::text(input, "group").unwrap_or(DEFAULT_GROUP);
            self.allowed(caller, group, name).await?;
            self.fire_now_for(caller, group, name).await
        })
    }
}

impl Scheduler {
    /// `scheduler:UpdateSchedule` on the schedule, as the call would need.
    async fn allowed(&self, caller: &Caller, group: &str, name: &str) -> Result<(), ApiError> {
        let Some(peers) = self.shared.peers.clone().filter(|p| p.mode() != Mode::Off) else {
            return Ok(());
        };
        let arn = schedule_arn(&caller.region, &caller.account_id, group, name);
        peers
            .authorize(caller, vec![Need::new("scheduler:UpdateSchedule", arn)])
            .await
    }

    async fn fire_now_for(
        &self,
        caller: &Caller,
        group: &str,
        name: &str,
    ) -> Result<Step, ApiError> {
        let ask = Request::Force {
            account: caller.account_id.clone(),
            group: group.to_owned(),
            name: name.to_owned(),
        };
        let (answer, mut fired) = self.ask(&caller.region, ask).await?;
        let left = match answer {
            Ok(left) => left,
            Err(e) if e.code == NOTHING => return Ok(Step::Nothing(e.message)),
            Err(e) => return Err(e),
        };
        let Some(inv) = fired.pop() else {
            return Err(ApiError::internal("the schedule fired nothing"));
        };
        let label = if group == DEFAULT_GROUP {
            name.to_owned()
        } else {
            format!("{group}/{name}")
        };
        let first = self.first_attempt(inv.clone()).await;
        let mut sentence = format!("Fired {label} now: {}", what_happened(&inv, first.as_ref()));
        sentence.push_str(&afterwards(&left));
        Ok(Step::Done(Done {
            sentence,
            subject: format!("{group}/{name}"),
        }))
    }

    /// Makes the first attempt in a task of its own, which goes on to the retries as a scheduled invocation does, and waits for how
    /// the first came out; `None` when it has not answered in [`FIRST_ATTEMPT`] (the task carries on).
    async fn first_attempt(&self, inv: Invocation) -> Option<Outcome> {
        let (tx, rx) = oneshot::channel();
        let sh = std::sync::Arc::clone(&self.shared);
        self.shared.tasks.spawn(async move {
            let first = attempt(&sh, &inv, 1).await;
            let told = match &first {
                Outcome::Sent => Outcome::Sent,
                Outcome::NotLocal(why) => Outcome::NotLocal(why.clone()),
                Outcome::Failed(f) => Outcome::Failed(f.clone()),
            };
            if tx.send(told).is_err() {
                // WHY: the Lab stopped waiting; the invocation goes on all the same.
            }
            crate::retry::carry_on(sh, inv, 1, first).await;
        });
        tokio::time::timeout(FIRST_ATTEMPT, rx).await.ok()?.ok()
    }
}

/// What the first attempt came to, in words.
fn what_happened(inv: &Invocation, first: Option<&Outcome>) -> String {
    let arn = inv
        .target
        .get("Arn")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let target = target_words(arn);
    match first {
        Some(Outcome::Sent) => format!("it {}.", target.done),
        Some(Outcome::NotLocal(why)) => format!(
            "nothing here can deliver to {} ({why}). It is not retried, as a scheduled one is not.",
            target.name
        ),
        Some(Outcome::Failed(f)) => format!(
            "the target {} refused it with {}: {} {}",
            target.name,
            f.code,
            f.message,
            if f.retryable {
                "The schedule's retry policy carries on from here, then its dead-letter queue if it has one, as it does for a scheduled invocation."
            } else {
                "That is not retried, as a scheduled one is not: the target refused it as wrong. It goes to the schedule's dead-letter queue if it has one."
            }
        ),
        None => format!(
            "the first attempt to {} has not answered yet; it carries on in the background.",
            target.name
        ),
    }
}

/// How a target reads in a sentence: its name, and what the invocation did to it.
struct Target {
    name: String,
    done: String,
}

fn target_words(arn: &str) -> Target {
    let resource = arn
        .split(':')
        .nth(6)
        .filter(|_| arn.split(':').nth(5) == Some("function"))
        .or_else(|| arn.rsplit([':', '/']).next())
        .unwrap_or(arn);
    let (name, done) = match target_kind(arn) {
        Some(Kind::Templated { service: "sqs" }) => (
            format!("{resource} (an SQS queue)"),
            format!("sent a message to {resource}"),
        ),
        Some(Kind::Templated { service: "sns" }) => (
            format!("{resource} (an SNS topic)"),
            format!("published to {resource}"),
        ),
        Some(Kind::Templated { service: "lambda" }) => (
            format!("{resource} (a Lambda function)"),
            format!("invoked {resource} asynchronously"),
        ),
        Some(Kind::Templated { service: "states" }) => (
            format!("{resource} (a Step Functions state machine)"),
            format!("started an execution of {resource}"),
        ),
        Some(Kind::Templated { service: "events" }) => (
            format!("{resource} (an EventBridge bus)"),
            format!("put an event on {resource}"),
        ),
        Some(Kind::Templated { service: "kinesis" }) => (
            format!("{resource} (a Kinesis stream)"),
            format!("put a record on {resource}"),
        ),
        Some(Kind::Universal { sdk, action }) => (
            format!("{sdk}:{action} (a universal target)"),
            format!("called {sdk}:{action}"),
        ),
        _ => (resource.to_owned(), format!("sent to {resource}")),
    };
    Target { name, done }
}

/// What the schedule has left after the fire.
fn afterwards(left: &Value) -> String {
    if left.get("removed").and_then(Value::as_bool) == Some(true) {
        return " That was its one fire, and the schedule is deleted because its ActionAfterCompletion is DELETE.".to_owned();
    }
    if left.get("once").and_then(Value::as_bool) == Some(true) {
        return " That was its one fire.".to_owned();
    }
    match left.get("next_ms").and_then(Value::as_i64) {
        Some(ms) => format!(" Its next scheduled fire is unchanged: {}.", utc(ms)),
        None => String::new(),
    }
}

/// `2026-01-02 09:00 UTC`.
fn utc(ms: i64) -> String {
    let iso = aws_core::time::format_iso8601(ms.div_euclid(1000));
    format!("{} UTC", iso.get(..16).unwrap_or(&iso).replace('T', " "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_named_by_what_they_are() {
        let t = target_words("arn:aws:sqs:us-east-1:000000000000:email-outbox");
        assert_eq!(t.name, "email-outbox (an SQS queue)");
        assert_eq!(t.done, "sent a message to email-outbox");
        let t = target_words("arn:aws:lambda:us-east-1:000000000000:function:report:live");
        assert_eq!(t.name, "report (a Lambda function)");
        let t = target_words("arn:aws:scheduler:::aws-sdk:sqs:sendMessage");
        assert_eq!(t.name, "sqs:sendMessage (a universal target)");
    }

    #[test]
    fn a_time_is_read_in_utc() {
        assert_eq!(
            utc(1_767_225_600_000 + 9 * 3_600_000),
            "2026-01-01 09:00 UTC"
        );
    }
}
