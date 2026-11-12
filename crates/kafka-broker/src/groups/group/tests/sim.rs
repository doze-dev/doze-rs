//! What the two ACL simulations share (`liveness`, `invariants`): a fixed topic list, a seeded generator, ACL records and
//! the rights they give, a classic subscription, and the pump that feeds a group events and collects its effects.

use std::collections::{BTreeSet, HashMap};

use bytes::{BufMut, Bytes, BytesMut};
use kafka_auth::Principal;
use kafka_meta::AclRecord;

use super::*;
use crate::Caller;
use crate::authorizer::{Authorizer, TopicRights, permits};
use crate::groups::consumer::protocol::ByName;
use crate::groups::consumer::{TopicSource, Topics};
use crate::groups::machine::{Answer, Effect, Event, Millis, TimerKey, TimerKind, Token};

pub(super) struct Fixed(pub(super) Topics);

impl TopicSource for Fixed {
    fn topics(&self) -> Topics {
        self.0.clone()
    }
}

/// A xorshift generator: the same seed is the same run.
pub(super) struct Rng(pub(super) u64);

impl Rng {
    pub(super) fn from_seed(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub(super) fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() >> 11).unwrap() % n
    }

    pub(super) fn pct(&mut self, p: usize) -> bool {
        self.below(100) < p
    }
}

pub(super) fn mid(s: &str) -> MemberId {
    MemberId::new(s).unwrap()
}

/// An ACL record on a topic: literal (3) or prefixed (4), for `User:who`, DESCRIBE, allowed or denied.
pub(super) fn describe_record(name: &str, pattern: i8, who: &str, allow: bool) -> AclRecord {
    AclRecord {
        resource_type: 2,
        name: name.into(),
        pattern_type: pattern,
        principal: format!("User:{who}"),
        host: "*".into(),
        operation: 8,
        permission: if allow { 3 } else { 2 },
    }
}

/// The DESCRIBE rights each of these users has under the records (`None`: unnarrowed).
pub(super) fn rights_of(
    records: Vec<AclRecord>,
    default_allow: bool,
    supers: &[&str],
    users: &[&str],
) -> Vec<Option<TopicRights>> {
    let supers: BTreeSet<String> = supers.iter().map(|u| format!("User:{u}")).collect();
    let a = Authorizer::new(records, default_allow, supers);
    users
        .iter()
        .map(|u| a.describe_rights(&Caller::new(Principal::user(u), None)))
        .collect()
}

pub(super) fn may(rights: &Option<TopicRights>, topic: &str) -> bool {
    permits(rights.as_ref(), topic)
}

/// A classic subscription (a consumer-protocol `version`) to these topics, owning these partitions.
pub(super) fn subscription_of(version: i16, topics: &[String], owned: &ByName) -> Bytes {
    let mut b = BytesMut::new();
    b.put_i16(version);
    b.put_i32(i32::try_from(topics.len()).unwrap());
    for t in topics {
        b.put_i16(i16::try_from(t.len()).unwrap());
        b.put_slice(t.as_bytes());
    }
    if version >= 1 {
        b.put_i32(-1);
        b.put_i32(i32::try_from(owned.len()).unwrap());
        for (n, ps) in owned {
            b.put_i16(i16::try_from(n.len()).unwrap());
            b.put_slice(n.as_bytes());
            b.put_i32(i32::try_from(ps.len()).unwrap());
            for p in ps {
                b.put_i32(*p);
            }
        }
    }
    b.freeze()
}

pub(super) fn made_ids() -> IdSource {
    let mut n = 0;
    Box::new(move |_| {
        n += 1;
        MemberId::new(&format!("made-{n}")).unwrap()
    })
}

pub(super) fn settings_of(assign_interval_ms: u64) -> Settings {
    Settings {
        initial_delay_ms: 0,
        consumer: consumer::Config {
            session_ms: 45_000,
            assign_interval_ms,
        },
        ids: made_ids,
    }
}

/// A consumer group, its clock, its timers and the answers it has given.
pub(super) struct Pump {
    pub(super) group: Group,
    pub(super) now: u64,
    next: u64,
    timers: HashMap<TimerKind, (u64, TimerKey)>,
    answers: HashMap<u64, Answer>,
}

impl Pump {
    pub(super) fn new(assign_interval_ms: u64) -> Self {
        Self {
            group: Group::consumer(settings_of(assign_interval_ms)),
            now: 1_000,
            next: 0,
            timers: HashMap::new(),
            answers: HashMap::new(),
        }
    }

    /// Sends one event at the clock's time; the token it was sent with.
    pub(super) fn send(&mut self, topics: &Topics, event: impl FnOnce(Token) -> Event) -> u64 {
        self.next += 1;
        let token = Token(self.next);
        let src = Fixed(topics.clone());
        for e in self.group.step(event(token), Millis(self.now), &src) {
            match e {
                Effect::Reply(t, a) => {
                    self.answers.insert(t.0, a);
                }
                Effect::Arm { key, at } => {
                    self.timers.insert(key.kind.clone(), (at.0, key));
                }
                Effect::Disarm(k) => {
                    self.timers.remove(&k);
                }
                Effect::Forget(_) => {}
            }
        }
        token.0
    }

    /// The answer to a token, if the group has given it (a parked request has none yet).
    pub(super) fn take(&mut self, token: u64) -> Option<Answer> {
        self.answers.remove(&token)
    }

    /// Fires the earliest timer due at or before `until` (the clock goes to its time). `false`: none was due.
    pub(super) fn fire_next(&mut self, topics: &Topics, until: u64) -> bool {
        let due = self
            .timers
            .values()
            .filter(|(at, _)| *at <= until)
            .min_by_key(|(at, key)| (*at, key.generation, format!("{:?}", key.kind)))
            .map(|(at, key)| (*at, key.clone()));
        let Some((at, key)) = due else { return false };
        self.timers.remove(&key.kind);
        self.now = at;
        self.send(topics, |_| Event::Timer(key));
        true
    }

    /// Every timer due in the next `ms` fires in order, then the clock is there.
    pub(super) fn advance(&mut self, topics: &Topics, ms: u64) {
        let until = self.now + ms;
        while self.fire_next(topics, until) {}
        self.now = until;
    }
}
