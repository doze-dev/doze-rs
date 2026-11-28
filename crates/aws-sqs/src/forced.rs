//! Forced events (the console's Lab): the messages of a queue that are in flight come back now.
//!
//! A message in flight is hidden until a time kept on it, and the clock alone ends that: there is no step of a timer to run. What a
//! consumer does to end it early is `ChangeMessageVisibility` with a timeout of 0 and the receipt handle it was given, and that
//! is what this runs, through the function the call runs ([`Queue::change_visibility`]), for each message in flight with the latest
//! handle it was given. So the message is receivable again at once, with the receive count it had, its older handles still naming
//! it, and the 12-hour limit on a message's visibility is not touched. A receive that finds one past its queue's `maxReceiveCount`
//! moves it to the dead-letter queue, as it does when the timeout runs out.
//!
//! The authorizer is asked for `sqs:ChangeMessageVisibility` on the queue, as the call is.

use aws_peers::{Mode, Need};
use aws_wire::ApiError;
use aws_wire::forced::{self, Done, Forced, Step, kind};
use aws_wire::service::{BoxFuture, Caller};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::Sqs;
use crate::core::{Ask, Core, no_such_queue};
use crate::policy::arn;

/// A forced event and where its answer goes.
#[derive(Debug)]
pub(crate) struct Release {
    pub(crate) caller: Caller,
    pub(crate) queue: String,
    pub(crate) reply: oneshot::Sender<Result<Step, ApiError>>,
}

impl Core {
    /// Carries the event out on the actor and tells whoever asked; the caller of this answers as any ask does (what it changed is
    /// flushed).
    pub(crate) async fn release(&mut self, ask: Release) {
        let Release {
            caller,
            queue,
            reply,
        } = ask;
        let done = match self.allowed(&caller, &queue).await {
            Ok(()) => self.release_in_flight(&queue),
            Err(refusal) => Err(refusal),
        };
        if reply.send(done).is_err() {
            // WHY: the person stopped waiting; what was asked was done all the same.
        }
    }

    /// The right the call would need: `sqs:ChangeMessageVisibility` on the queue, with its policy.
    async fn allowed(&self, caller: &Caller, queue: &str) -> Result<(), ApiError> {
        let Some(peers) = self.peers.clone().filter(|p| p.mode() != Mode::Off) else {
            return Ok(());
        };
        let policy = self
            .queues
            .get(queue)
            .and_then(|q| q.stored.get("Policy").cloned());
        let need = Need::new(
            "sqs:ChangeMessageVisibility",
            arn(&caller.region, &caller.account_id, queue),
        )
        .with_policy(policy);
        peers.authorize(caller, vec![need]).await
    }

    pub(crate) fn release_in_flight(&mut self, name: &str) -> Result<Step, ApiError> {
        let now = self.now_ms();
        let queue = self.queue_named(name).ok_or_else(no_such_queue)?;
        queue.expire(now, false);
        let held: Vec<(String, String, u32)> = queue
            .messages
            .iter()
            .filter(|m| m.in_flight(now))
            .filter_map(|m| Some((m.handles.last()?.clone(), m.id.clone(), m.receive_count)))
            .collect();
        if held.is_empty() {
            let delayed = queue.messages.iter().filter(|m| m.delayed(now)).count();
            let waiting = queue.messages.len().saturating_sub(delayed);
            return Ok(Step::Nothing(nothing(name, delayed, waiting)));
        }
        let mut counts = Vec::new();
        for (handle, id, count) in &held {
            queue.change_visibility(handle, 0, now)?;
            counts.push((id.as_str(), *count));
        }
        let limit = queue.redrive().map(|r| r.max_receive_count);
        let past = limit.map_or(0, |max| counts.iter().filter(|(_, n)| *n >= max).count());
        Ok(Step::Done(Done {
            sentence: sentence(name, &counts, past),
            subject: name.to_owned(),
        }))
    }
}

/// Why there was nothing to bring back, and what the queue holds instead.
fn nothing(queue: &str, delayed: usize, waiting: usize) -> String {
    let mut s = format!(
        "{queue} has no message in flight: none has been received and left without a delete, so no visibility timeout is running."
    );
    if waiting > 0 {
        s.push_str(&format!(" {} can be received now.", many(waiting)));
    }
    if delayed > 0 {
        s.push_str(&format!(
            " {} waiting out a delivery delay, which is not a visibility timeout and ends by itself.",
            many(delayed)
        ));
    }
    s
}

/// `1 message`, `3 messages`.
fn many(n: usize) -> String {
    if n == 1 {
        "1 message".to_owned()
    } else {
        format!("{n} messages")
    }
}

/// What was done, with each message's receive count (the first few, by id).
fn sentence(queue: &str, held: &[(&str, u32)], past: usize) -> String {
    let n = held.len();
    let mut shown: Vec<String> = held
        .iter()
        .take(3)
        .map(|(id, count)| {
            let times = if *count == 1 { "time" } else { "times" };
            format!("{}, received {count} {times}", short(id))
        })
        .collect();
    if n > shown.len() {
        shown.push(format!("and {} more", n - shown.len()));
    }
    let mut s = format!(
        "Brought back {} that {} in flight in {queue}: {} can be received again and {} the receive {} {} had ({}).",
        many(n),
        if n == 1 { "was" } else { "were" },
        if n == 1 { "it" } else { "they" },
        if n == 1 { "keeps" } else { "keep" },
        if n == 1 { "count" } else { "counts" },
        if n == 1 { "it" } else { "they" },
        shown.join("; "),
    );
    if past > 0 {
        s.push_str(&format!(
            " {} reached the queue's maxReceiveCount: the next receive moves {} to the dead-letter queue.",
            if past == 1 {
                "1 has".to_owned()
            } else {
                format!("{past} have")
            },
            if past == 1 { "it" } else { "them" },
        ));
    }
    s
}

/// The first eight characters of a message id.
fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

impl Forced for Sqs {
    fn force<'a>(
        &'a self,
        caller: &'a Caller,
        what: &'a str,
        input: &'a Value,
    ) -> BoxFuture<'a, Result<Step, ApiError>> {
        Box::pin(async move {
            if what != kind::EXPIRE_VISIBILITY {
                return Err(ApiError::internal(format!(
                    "SQS has no forced event {what}"
                )));
            }
            let queue = forced::text(input, "queue").ok_or_else(|| forced::missing("a queue"))?;
            self.release(caller, queue).await
        })
    }
}

impl Sqs {
    /// Asks the region's actor to bring the queue's in-flight messages back, saves what it changed, and wakes the receives that wait.
    async fn release(&self, caller: &Caller, queue: &str) -> Result<Step, ApiError> {
        let actor = self
            .actors
            .actor(&caller.region)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let (reply, answer) = oneshot::channel();
        let ask = Ask::Release(Box::new(Release {
            caller: caller.clone(),
            queue: queue.to_owned(),
            reply,
        }));
        let mut asked = actor
            .call(ask)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        if let Some(saved) = asked.durable.take()
            && let Err(e) = saved.await
        {
            return Err(ApiError::internal(format!("could not save: {e}")));
        }
        self.expiry.arm(&caller.region, &actor, asked.next_due);
        // A receive that waits for a message may find these.
        self.tell(&caller.region, &asked);
        answer
            .await
            .map_err(|_| ApiError::internal("the queue's actor went away"))?
    }
}
