//! The task that owns one group's machine: it turns requests into events, timers into events, and effects into
//! answers on the channels the requests came with.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, sleep_until};

use super::machine::{
    Answer, CommitRequest, Effect, Event, GroupMachine, HeartbeatRequest, JoinRequest, LeaveTarget,
    Millis, SyncRequest, TimerKey, TimerKind, Token,
};

/// A request for the group.
pub(crate) enum Req {
    Join(JoinRequest),
    Sync(SyncRequest),
    Heartbeat(HeartbeatRequest),
    Leave(Vec<LeaveTarget>),
    Commit(CommitRequest),
    Describe,
    Delete,
}

/// A request and where its answer goes. If the sender is dropped without an answer the request was replaced by a
/// later one and will never be answered.
pub(super) struct Command {
    pub(super) req: Req,
    pub(super) reply: oneshot::Sender<Answer>,
}

/// The timers the machine has asked for, in the order they run out.
#[derive(Default)]
struct Timers {
    by_time: BTreeMap<(Millis, u64), TimerKey>,
    by_kind: HashMap<TimerKind, (Millis, u64)>,
    arming: u64,
}

impl Timers {
    fn arm(&mut self, key: TimerKey, at: Millis) {
        self.disarm(&key.kind);
        self.arming += 1;
        self.by_kind.insert(key.kind.clone(), (at, self.arming));
        self.by_time.insert((at, self.arming), key);
    }

    fn disarm(&mut self, kind: &TimerKind) {
        if let Some(slot) = self.by_kind.remove(kind) {
            self.by_time.remove(&slot);
        }
    }

    /// When the first runs out.
    fn next(&self) -> Option<Millis> {
        self.by_time.keys().next().map(|(at, _)| *at)
    }

    /// The first, if it has run out by `now`.
    fn pop_due(&mut self, now: Millis) -> Option<TimerKey> {
        let slot = *self.by_time.keys().next().filter(|(at, _)| *at <= now)?;
        let key = self.by_time.remove(&slot)?;
        self.by_kind.remove(&key.kind);
        Some(key)
    }
}

/// Runs the group until every sender of commands is gone.
pub(super) async fn run(mut machine: GroupMachine, mut commands: mpsc::Receiver<Command>) {
    let start = Instant::now();
    let now = || Millis(u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX));
    let mut waiting: HashMap<Token, oneshot::Sender<Answer>> = HashMap::new();
    let mut timers = Timers::default();
    let mut tokens = 0_u64;
    // A group deleted while empty is gone: the registry has taken it out, and nothing is left to run.
    let mut deleted = false;
    loop {
        if deleted {
            break;
        }
        let wake = timers.next().map(|at| start + Duration::from_millis(at.0));
        let event = tokio::select! {
            command = commands.recv() => {
                let Some(Command { req, reply }) = command else { break };
                tokens += 1;
                let token = Token(tokens);
                waiting.insert(token, reply);
                event_of(token, req)
            }
            () = sleep_or_wait_forever(wake) => match timers.pop_due(now()) {
                Some(key) => Event::Timer(key),
                None => continue,
            },
        };
        for effect in machine.step(event, now()) {
            match effect {
                Effect::Reply(token, answer) => {
                    deleted |= matches!(answer, Answer::Delete(None));
                    if let Some(reply) = waiting.remove(&token) {
                        // A client that went away is not told, and the group does not mind.
                        let _ = reply.send(answer);
                    }
                }
                Effect::Forget(token) => {
                    waiting.remove(&token);
                }
                Effect::Arm { key, at } => timers.arm(key, at),
                Effect::Disarm(kind) => timers.disarm(&kind),
            }
        }
    }
}

async fn sleep_or_wait_forever(until: Option<Instant>) {
    match until {
        Some(at) => sleep_until(at).await,
        None => std::future::pending().await,
    }
}

fn event_of(token: Token, req: Req) -> Event {
    match req {
        Req::Join(r) => Event::Join(token, r),
        Req::Sync(r) => Event::Sync(token, r),
        Req::Heartbeat(r) => Event::Heartbeat(token, r),
        Req::Leave(t) => Event::Leave(token, t),
        Req::Commit(r) => Event::Commit(token, r),
        Req::Describe => Event::Describe(token),
        Req::Delete => Event::Delete(token),
    }
}
