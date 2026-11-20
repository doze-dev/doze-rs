//! `SubscribeToShard`: an event stream (`application/vnd.amazon.eventstream`) that pushes a shard's records to a registered consumer.
//! What the model documents, kept: events "for up to 5 minutes, after which time you need to call `SubscribeToShard` again", and a
//! later call for the same consumer and shard "takes over the subscription and the previous connection expires or fails with a
//! `ResourceInUseException`". The model also says a call "within 5 seconds of a successful call" is refused; AWS did not refuse one
//! made at once (recorded 2026-11-19), so every call takes over.
//!
//! A task per open subscription (none while there is none) writes the frames: an `initial-response` first (botocore's JSON parser
//! reads it before the members), an event at once, then one whenever records arrive (it waits on the service's append counter, not
//! a poll) and at least every five seconds when none do (memory: AWS's heartbeat, which carries the continuation). Its two timers,
//! the heartbeat and the end of its five minutes, are one [`Expiring`]. Each read is the actor's ([`Core::fanout_read`]), so the
//! subscription sees a shard as `GetRecords` would.

use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};
use std::time::Duration;

use aws_wire::ApiError;
use aws_wire::eventstream::{event, exception};
use aws_wire::rest::{ByteStream, Download};
use bytes::Bytes;
use doze_runtime::{Expiring, Lazy, Tasks};
use serde_json::json;
use tokio::sync::{mpsc, oneshot, watch};

use crate::core::{Ask, Core};
use crate::fanout::{Batch, FanoutAsk, Position};

/// The model: events flow "for up to 5 minutes".
const LIFE: Duration = Duration::from_secs(300);
/// An event at least this often while no record arrives (memory).
const HEARTBEAT: Duration = Duration::from_secs(5);

/// The open subscriptions of the process, by region, consumer and shard.
#[derive(Debug, Default)]
pub(crate) struct Subscriptions {
    live: Mutex<HashMap<String, Live>>,
    ids: AtomicU64,
}

#[derive(Debug)]
struct Live {
    id: u64,
    takeover: oneshot::Sender<()>,
}

/// A claim on a consumer's shard: released when its subscription ends.
#[derive(Debug)]
pub(crate) struct Claim {
    subs: Arc<Subscriptions>,
    key: String,
    id: u64,
}

impl Drop for Claim {
    fn drop(&mut self) {
        let mut live = self
            .subs
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if live.get(&self.key).is_some_and(|l| l.id == self.id) {
            live.remove(&self.key);
        }
    }
}

impl Subscriptions {
    /// Claims the consumer's shard, ending an older subscription to it. The receiver hears when a later call takes this one over.
    pub(crate) fn claim(
        self: &Arc<Self>,
        region: &str,
        p: &Position,
    ) -> (Claim, oneshot::Receiver<()>) {
        let key = format!("{region}\u{1}{}\u{1}{}", p.consumer_arn, p.shard);
        let mut live = self.live.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(old) = live.remove(&key)
            && old.takeover.send(()).is_err()
        {
            // WHY: the older subscription had already ended; there is nobody to tell.
        }
        let (tx, rx) = oneshot::channel();
        let id = self.ids.fetch_add(1, Ordering::Relaxed);
        live.insert(key.clone(), Live { id, takeover: tx });
        let claim = Claim {
            subs: Arc::clone(self),
            key,
            id,
        };
        (claim, rx)
    }

    /// How many subscriptions are open (for tests).
    #[cfg(test)]
    pub(crate) fn open(&self) -> usize {
        self.live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

/// What a subscription's task holds.
pub(crate) struct Session {
    pub(crate) actor: Arc<Lazy<Core>>,
    pub(crate) account: String,
    pub(crate) position: Position,
    pub(crate) appended: watch::Receiver<u64>,
    /// Held for its drop: the consumer's shard is free again when the session ends.
    pub(crate) _claim: Claim,
    pub(crate) takeover: oneshot::Receiver<()>,
}

/// The two timers of a subscription.
#[derive(Debug, Clone, Copy)]
enum Tick {
    Beat,
    End,
}

/// The frames of a subscription, as the gateway pulls them.
struct Frames(mpsc::Receiver<io::Result<Bytes>>);

impl ByteStream for Frames {
    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<io::Result<Bytes>>> {
        self.0.poll_recv(cx)
    }
}

/// The answer's body: the subscription's frames, written by a task that ends with the subscription.
pub(crate) fn stream(tasks: &Tasks, session: Session) -> Download {
    let (tx, rx) = mpsc::channel::<io::Result<Bytes>>(4);
    let stopping = tasks.stopping();
    tasks.spawn(async move {
        tokio::select! {
            () = stopping.cancelled() => {}
            () = session.run(&tx) => {}
        }
    });
    Box::pin(Frames(rx))
}

async fn send(tx: &mpsc::Sender<io::Result<Bytes>>, frame: Vec<u8>) -> bool {
    tx.send(Ok(Bytes::from(frame))).await.is_ok()
}

/// The frame a subscription ends with.
async fn last(tx: &mpsc::Sender<io::Result<Bytes>>, frame: Vec<u8>) {
    if !send(tx, frame).await {
        // WHY: the reader has gone; there is nobody to tell why the subscription ended.
    }
}

/// An error sent in the stream: one the union carries by name, else `InternalFailureException`.
fn failure(e: &ApiError) -> Vec<u8> {
    let name = e.shape.as_deref().unwrap_or_default();
    let carried = [
        "ResourceNotFoundException",
        "ResourceInUseException",
        "InternalFailureException",
    ];
    let name = if carried.contains(&name) {
        name
    } else {
        "InternalFailureException"
    };
    exception(name, json!({"message": e.message}).to_string().as_bytes())
}

/// What a subscription that a later call took over ends with (the model names the exception; the words are memory).
fn taken_over() -> Vec<u8> {
    let body = json!({"message": "The subscription was taken over by a later SubscribeToShard call for the same consumer and shard."});
    exception("ResourceInUseException", body.to_string().as_bytes())
}

impl Session {
    async fn read(&self) -> Result<Batch, ApiError> {
        let (reply, answer) = oneshot::channel();
        let ask = Ask::Fanout(Box::new(FanoutAsk::Read(
            self.position.clone(),
            self.account.clone(),
            reply,
        )));
        self.actor
            .call(ask)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        answer
            .await
            .map_err(|_| ApiError::internal("the read was not answered"))?
    }

    async fn run(mut self, tx: &mpsc::Sender<io::Result<Bytes>>) {
        if !send(tx, event("initial-response", "application/json", b"{}")).await {
            return;
        }
        let mut timers = Expiring::default();
        timers.insert(Tick::End, LIFE);
        let mut beat = timers.insert(Tick::Beat, HEARTBEAT);
        let mut due = true;
        loop {
            if self.takeover.try_recv().is_ok() {
                last(tx, taken_over()).await;
                return;
            }
            let batch = match self.read().await {
                Ok(b) => b,
                Err(e) => {
                    last(tx, failure(&e)).await;
                    return;
                }
            };
            self.position.next = batch.next;
            if batch.records > 0 || batch.ended || due {
                let body = batch.event.to_string();
                if !send(
                    tx,
                    event("SubscribeToShardEvent", "application/json", body.as_bytes()),
                )
                .await
                {
                    return;
                }
                due = false;
                beat = timers
                    .reset(&beat, HEARTBEAT)
                    .unwrap_or_else(|| timers.insert(Tick::Beat, HEARTBEAT));
            }
            if batch.ended {
                return;
            }
            if batch.records > 0 {
                continue;
            }
            tokio::select! {
                () = tx.closed() => return,
                taken = &mut self.takeover => {
                    if taken.is_ok() {
                        last(tx, taken_over()).await;
                    }
                    return;
                }
                changed = self.appended.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
                tick = timers.expired() => match tick {
                    Tick::End => return,
                    Tick::Beat => due = true,
                },
            }
        }
    }
}
