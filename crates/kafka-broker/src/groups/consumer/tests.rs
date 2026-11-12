//! The consumer group, driven by a number for a clock and a map for timers: no sleeps, no tasks.

mod fetch;
mod ownership;
mod protocol;
mod simulation;

use std::collections::BTreeMap;

use kafka_wire::Uuid;
use kafka_wire::ids::{InstanceId, MemberId};

use super::assign::Partitions;
use super::*;
use crate::groups::machine::{Answer, Token};

pub(super) const T: Uuid = Uuid::from_u128(1);
pub(super) const SESSION: u64 = 6_000;
pub(super) const INTERVAL: u64 = 1_000;

pub(super) fn topics(partitions: i32) -> Topics {
    Topics::new([TopicInfo {
        name: "t".into(),
        id: T,
        partitions,
    }])
}

pub(super) fn parts(ps: &[i32]) -> Partitions {
    ps.iter().map(|p| (T, *p)).collect()
}

pub(super) fn id(s: &str) -> MemberId {
    MemberId::new(s).unwrap()
}

/// A group with the clock and timers whoever drives it would keep.
pub(super) struct Rig {
    pub(super) group: ConsumerGroup,
    pub(super) now: u64,
    pub(super) topics: Topics,
    timers: std::collections::HashMap<TimerKind, (u64, TimerKey)>,
    next_token: u64,
}

pub(super) fn rig(partitions: i32) -> Rig {
    let mut n = 0;
    let new_id: IdSource = Box::new(move |_| {
        n += 1;
        MemberId::new(&format!("made-{n}")).unwrap()
    });
    Rig {
        group: ConsumerGroup::new(
            Config {
                session_ms: SESSION,
                assign_interval_ms: INTERVAL,
            },
            new_id,
        ),
        now: 0,
        topics: topics(partitions),
        timers: std::collections::HashMap::new(),
        next_token: 0,
    }
}

/// A heartbeat that joins, subscribing to `t`.
pub(super) fn join(member: &str) -> Heartbeat {
    Heartbeat {
        group: "g".into(),
        member_id: MemberId::new(member),
        version: 1,
        epoch: 0,
        instance: None,
        rack: None,
        rebalance_ms: 60_000,
        subscribed: Some(vec!["t".into()]),
        regex: None,
        assignor: None,
        owned: Some(vec![]),
        client_id: "client".into(),
        client_host: "/127.0.0.1".into(),
        rights: None,
    }
}

/// A heartbeat that keeps on at `epoch`, saying nothing of what it owns or subscribes to.
pub(super) fn poll(member: &str, epoch: i32) -> Heartbeat {
    Heartbeat {
        epoch,
        subscribed: None,
        owned: None,
        ..join(member)
    }
}

/// A heartbeat that keeps on at `epoch`, saying what it owns.
pub(super) fn poll_owning(member: &str, epoch: i32, owned: &[i32]) -> Heartbeat {
    Heartbeat {
        owned: Some(vec![(T, owned.to_vec())]),
        ..poll(member, epoch)
    }
}

impl Rig {
    pub(super) fn beat(&mut self, h: Heartbeat) -> HeartbeatAnswer {
        self.next_token += 1;
        let token = Token(self.next_token);
        let mut out = Vec::new();
        self.group
            .heartbeat(token, &h, &self.topics, Millis(self.now), &mut out);
        let mut answer = None;
        for effect in out {
            match effect {
                Effect::Reply(t, Answer::ConsumerHeartbeat(a)) if t == token => answer = Some(a),
                other => self.apply(other),
            }
        }
        answer.expect("every heartbeat is answered")
    }

    fn apply(&mut self, effect: Effect) {
        match effect {
            Effect::Arm { key, at } => {
                self.timers.insert(key.kind.clone(), (at.0, key));
            }
            Effect::Disarm(kind) => {
                self.timers.remove(&kind);
            }
            Effect::Reply(..) | Effect::Forget(_) => {}
        }
    }

    /// Lets `ms` pass, running the timers that fall due in the order they do.
    pub(super) fn advance(&mut self, ms: u64) {
        let until = self.now + ms;
        loop {
            let due = self
                .timers
                .values()
                .filter(|(at, _)| *at <= until)
                .min_by_key(|(at, key)| (*at, key.generation))
                .map(|(at, key)| (*at, key.clone()));
            let Some((at, key)) = due else { break };
            self.timers.remove(&key.kind);
            self.now = at;
            let mut out = Vec::new();
            self.group.timer(&key, Millis(self.now), &mut out);
            for effect in out {
                self.apply(effect);
            }
        }
        self.now = until;
    }

    pub(super) fn view(&self) -> ConsumerView {
        self.group.view()
    }

    pub(super) fn member(&self, id: &str) -> MemberView {
        self.view()
            .members
            .into_iter()
            .find(|m| m.id.as_str() == id)
            .unwrap_or_else(|| panic!("{id} is not a member"))
    }
}

pub(super) fn instance(s: &str) -> InstanceId {
    InstanceId::new(s)
}
