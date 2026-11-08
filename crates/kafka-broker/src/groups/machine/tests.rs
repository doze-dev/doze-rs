//! The group machine, driven by a number for a clock and a map for timers: no sleeps, no tasks.

mod membership;
mod properties;
mod rebalance;
mod static_members;
mod timers;

use std::collections::{HashMap, HashSet};

use bytes::Bytes;
use kafka_wire::ResponseError;
use kafka_wire::ids::{InstanceId, MemberId};

use super::*;

/// A machine with the clock and timers that whoever drives it would keep.
pub(super) struct Group {
    pub(super) machine: GroupMachine,
    pub(super) now: u64,
    next_token: u64,
    /// Answers not yet taken, by request.
    answers: HashMap<Token, Answer>,
    /// The timer of each kind that counts.
    timers: HashMap<TimerKind, (Millis, TimerKey)>,
    /// Requests other than a JoinGroup that have not been answered. (A JoinGroup sent again replaces the one
    /// waiting, which is never answered; every other request is answered once.)
    pub(super) unanswered: HashSet<Token>,
    /// Requests the machine said would never be answered.
    pub(super) forgotten: HashSet<Token>,
}

/// Makes ids `m1`, `m2`, ... (and `i-<instance>-n` for a static member).
fn ids() -> IdSource {
    let mut n = 0;
    Box::new(move |instance| {
        n += 1;
        let text = match instance {
            Some(i) => format!("{i}-{n}"),
            None => format!("m{n}"),
        };
        MemberId::new(&text).unwrap()
    })
}

pub(super) fn id(s: &str) -> MemberId {
    MemberId::new(s).unwrap()
}

pub(super) fn instance(s: &str) -> InstanceId {
    InstanceId::new(s).unwrap()
}

pub(super) fn protocols(names: &[&str]) -> Vec<Protocol> {
    names
        .iter()
        .map(|n| Protocol {
            name: (*n).into(),
            metadata: Bytes::from(format!("meta-{n}")),
        })
        .collect()
}

/// A JoinGroup of a consumer at the usual timeouts.
pub(super) fn join_request(member: Option<&str>, names: &[&str]) -> JoinRequest {
    JoinRequest {
        member_id: member.map(id),
        instance: None,
        protocol_type: "consumer".into(),
        protocols: protocols(names),
        session_ms: 10_000,
        rebalance_ms: 30_000,
        id_first: false,
    }
}

impl Group {
    pub(super) fn new(initial_delay_ms: u64) -> Self {
        Self::with_ids(initial_delay_ms, ids())
    }

    /// A group whose members are given the ids `names`, in order.
    pub(super) fn named(initial_delay_ms: u64, names: &[&'static str]) -> Self {
        let mut names = Vec::from(names).into_iter();
        let source: IdSource = Box::new(move |_| id(names.next().unwrap_or("overflow")));
        Self::with_ids(initial_delay_ms, source)
    }

    fn with_ids(initial_delay_ms: u64, source: IdSource) -> Self {
        Self {
            machine: GroupMachine::new(initial_delay_ms, source),
            now: 1_000,
            next_token: 0,
            answers: HashMap::new(),
            timers: HashMap::new(),
            unanswered: HashSet::new(),
            forgotten: HashSet::new(),
        }
    }

    /// Sends an event now and applies what comes back.
    pub(super) fn send(&mut self, event: impl FnOnce(Token) -> Event) -> Token {
        self.next_token += 1;
        let token = Token(self.next_token);
        let event = event(token);
        if !matches!(event, Event::Join(..)) {
            self.unanswered.insert(token);
        }
        let effects = self.machine.step(event, Millis(self.now));
        self.apply(effects);
        token
    }

    fn apply(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Reply(token, answer) => {
                    self.unanswered.remove(&token);
                    assert!(
                        self.answers.insert(token, answer).is_none(),
                        "{token:?} was answered twice"
                    );
                }
                Effect::Arm { key, at } => {
                    self.timers.insert(key.kind.clone(), (at, key));
                }
                Effect::Forget(token) => {
                    self.forgotten.insert(token);
                }
                Effect::Disarm(kind) => {
                    self.timers.remove(&kind);
                }
            }
        }
    }

    pub(super) fn join(&mut self, req: JoinRequest) -> Token {
        self.send(|t| Event::Join(t, req))
    }

    pub(super) fn sync(
        &mut self,
        member: &str,
        generation: i32,
        assignments: &[(&str, &str)],
    ) -> Token {
        let req = SyncRequest {
            member_id: id(member),
            instance: None,
            generation,
            protocol_type: None,
            protocol: None,
            assignments: assignments
                .iter()
                .map(|(m, a)| (id(m), Bytes::from((*a).to_owned())))
                .collect(),
        };
        self.send(|t| Event::Sync(t, req))
    }

    pub(super) fn heartbeat(&mut self, member: &str, generation: i32) -> Token {
        let req = HeartbeatRequest {
            member_id: id(member),
            instance: None,
            generation,
        };
        self.send(|t| Event::Heartbeat(t, req))
    }

    pub(super) fn leave(&mut self, members: &[&str]) -> Token {
        let targets = members
            .iter()
            .map(|m| LeaveTarget {
                member_id: Some(id(m)),
                instance: None,
            })
            .collect();
        self.send(|t| Event::Leave(t, targets))
    }

    /// Lets `ms` pass, firing the timers that run out on the way, earliest first.
    pub(super) fn advance(&mut self, ms: u64) {
        let target = self.now + ms;
        loop {
            let next = self
                .timers
                .iter()
                .filter(|(_, (at, _))| at.0 <= target)
                .min_by_key(|(_, (at, _))| *at)
                .map(|(kind, (at, key))| (kind.clone(), *at, key.clone()));
            let Some((kind, at, key)) = next else { break };
            self.timers.remove(&kind);
            self.now = at.0.max(self.now);
            let effects = self.machine.step(Event::Timer(key), Millis(self.now));
            self.apply(effects);
        }
        self.now = target;
    }

    /// Fires a timer key by hand, as a stale one that was already on its way would be.
    pub(super) fn fire(&mut self, key: TimerKey) {
        let effects = self.machine.step(Event::Timer(key), Millis(self.now));
        self.apply(effects);
    }

    /// The timer of this kind that counts, if one is set.
    pub(super) fn timer(&self, kind: &TimerKind) -> Option<(Millis, TimerKey)> {
        self.timers.get(kind).cloned()
    }

    /// The answer to a request, if it has been given; it can be taken once.
    pub(super) fn answer(&mut self, token: Token) -> Option<Answer> {
        self.answers.remove(&token)
    }

    pub(super) fn joined(&mut self, token: Token) -> JoinAnswer {
        match self.answer(token) {
            Some(Answer::Join(a)) => a,
            other => panic!("expected a join answer, found {other:?}"),
        }
    }

    pub(super) fn synced(&mut self, token: Token) -> SyncAnswer {
        match self.answer(token) {
            Some(Answer::Sync(a)) => a,
            other => panic!("expected a sync answer, found {other:?}"),
        }
    }

    pub(super) fn heartbeat_answer(&mut self, token: Token) -> Option<ResponseError> {
        match self.answer(token) {
            Some(Answer::Heartbeat(e)) => e,
            other => panic!("expected a heartbeat answer, found {other:?}"),
        }
    }
}

/// A group of `n` members (`m1`, `m2`, ...) that joined together, with the leader synced: stable, generation 1, under
/// the one protocol `range`. The initial delay is 100 ms.
pub(super) fn stable(n: usize) -> Group {
    let mut g = Group::new(100);
    let tokens: Vec<_> = (0..n)
        .map(|_| g.join(join_request(None, &["range"])))
        .collect();
    g.advance(500);
    let answers: Vec<_> = tokens.into_iter().map(|t| g.joined(t)).collect();
    let leader = answers[0].leader.clone().unwrap();
    let sync = g.sync(leader.as_str(), answers[0].generation, &[]);
    assert!(g.synced(sync).error.is_none());
    g
}
