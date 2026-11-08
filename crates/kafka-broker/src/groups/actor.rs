//! The task that owns one group's machine: it turns requests into events, timers into events, and effects into
//! answers on the channels the requests came with.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use kafka_wire::ResponseError;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, sleep_until};

use super::machine::{
    Answer, CommitRequest, Effect, Event, GroupMachine, HeartbeatRequest, JoinAnswer, JoinRequest,
    LeaveTarget, Millis, SyncRequest, TimerKey, TimerKind, Token,
};

/// A request for the group.
#[derive(Clone)]
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

/// Writes down what the offsets retention needs to know of a group's members.
pub(super) trait Records: Send + Sync + 'static {
    /// The group is about to have members, the longest session being `session_ms`.
    fn live(&self, session_ms: u32, protocol_type: &str) -> bool;
    /// The group has lost its last member, at `now_ms`.
    fn empty(&self, protocol_type: &str, now_ms: i64) -> bool;
}

/// How long after a failed write of the group's emptiness it is tried again.
const RETRY: Duration = Duration::from_millis(500);

/// What has been written down of the group, and what still has to be.
struct Keeper<R: Records> {
    records: R,
    /// The longest session written down.
    session: u32,
    /// The group has lost its last member and the record of that is not yet written; when to try again.
    unwritten_empty: Option<Instant>,
}

impl<R: Records> Keeper<R> {
    /// A member is not seated before the group's record says it has members, or its commits could be stored under a
    /// record that says nobody is there and be expired as an empty group's. `false`: the record could not be written.
    fn may_seat(&mut self, machine: &GroupMachine, req: &JoinRequest) -> bool {
        // A join that will be refused seats nobody and so needs no record (one would pin the group's offsets for ever).
        if !machine.would_seat(req) {
            return true;
        }
        let session = u32::try_from(req.session_ms).unwrap_or(0);
        if (machine.member_count() == 0 || session > self.session)
            && !self
                .records
                .live(session.max(self.session), &req.protocol_type)
        {
            return false;
        }
        self.session = self.session.max(session);
        true
    }

    /// The machine has taken an event: if that took its last member, say so; if it has members, the record that it
    /// had none is out of date.
    fn after(&mut self, machine: &GroupMachine, had_members: bool) {
        if had_members && machine.member_count() == 0 {
            self.write_empty(machine);
            self.session = 0;
        } else if machine.member_count() > 0 {
            self.unwritten_empty = None;
        }
    }

    fn write_empty(&mut self, machine: &GroupMachine) {
        let written = self
            .records
            .empty(machine.protocol_type().unwrap_or(""), wall_ms());
        self.unwritten_empty = (!written).then(|| Instant::now() + RETRY);
    }
}

/// Runs the group until every sender of commands is gone.
pub(super) async fn run(
    mut machine: GroupMachine,
    mut commands: mpsc::Receiver<Command>,
    records: impl Records,
) {
    let start = Instant::now();
    let now = || Millis(u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX));
    let mut waiting: HashMap<Token, oneshot::Sender<Answer>> = HashMap::new();
    let mut timers = Timers::default();
    let mut tokens = 0_u64;
    let mut keeper = Keeper {
        records,
        session: 0,
        unwritten_empty: None,
    };
    loop {
        let wake = timers.next().map(|at| start + Duration::from_millis(at.0));
        let event = tokio::select! {
            command = commands.recv() => {
                let Some(Command { req, reply }) = command else { break };
                tokens += 1;
                let token = Token(tokens);
                waiting.insert(token, reply);
                event_of(token, req)
            }
            () = sleep_or_wait_forever(keeper.unwritten_empty) => {
                keeper.write_empty(&machine);
                continue;
            }
            () = sleep_or_wait_forever(wake) => match timers.pop_due(now()) {
                Some(key) => Event::Timer(key),
                None => continue,
            },
        };
        if let Event::Join(token, req) = &event
            && !keeper.may_seat(&machine, req)
        {
            if let Some(reply) = waiting.remove(token) {
                let refused = JoinAnswer::refused(ResponseError::CoordinatorNotAvailable, None);
                let _ = reply.send(Answer::Join(refused));
            }
            continue;
        }
        let had_members = machine.member_count() > 0;
        let effects = machine.step(event, now());
        keeper.after(&machine, had_members);
        // A group deleted while empty is gone: the registry has taken it out, and nothing is left to run.
        if apply(effects, &mut waiting, &mut timers) {
            break;
        }
    }
}

/// Does what the machine asked. `true`: the group has been deleted.
fn apply(
    effects: Vec<Effect>,
    waiting: &mut HashMap<Token, oneshot::Sender<Answer>>,
    timers: &mut Timers,
) -> bool {
    let mut deleted = false;
    for effect in effects {
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
    deleted
}

/// Now, in milliseconds since the Unix epoch.
fn wall_ms() -> i64 {
    crate::clock::now().get()
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
