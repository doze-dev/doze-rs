//! A group changing kind: a classic group of consumers becomes a consumer group when a member of the new protocol
//! joins it, and is classic again when the last of those is gone.

use std::collections::HashMap;

use bytes::Bytes;
use kafka_wire::ResponseError as E;
use kafka_wire::Uuid;
use kafka_wire::ids::MemberId;

use super::super::consumer::protocol::{ByName, encode_assignment};
use super::super::consumer::{ConsumerView, Heartbeat, TopicInfo, TopicSource, Topics};
use super::super::machine::{
    Answer, Effect, Event, HeartbeatRequest, JoinRequest, Millis, Protocol, SyncRequest, TimerKey,
    TimerKind, Token,
};
use super::*;

const T: Uuid = Uuid::from_u128(1);

struct Fixed;

impl TopicSource for Fixed {
    fn topics(&self) -> Topics {
        Topics::new([
            TopicInfo {
                name: "t".into(),
                id: T,
                partitions: 2,
            },
            TopicInfo {
                name: "u".into(),
                id: Uuid::from_u128(2),
                partitions: 1,
            },
        ])
    }
}

fn ids() -> IdSource {
    let mut n = 0;
    Box::new(move |_| {
        n += 1;
        MemberId::new(&format!("made-{n}")).unwrap()
    })
}

fn settings() -> Settings {
    Settings {
        initial_delay_ms: 0,
        consumer: consumer::Config {
            session_ms: 45_000,
            assign_interval_ms: 0,
        },
        ids,
    }
}

fn id(s: &str) -> MemberId {
    MemberId::new(s).unwrap()
}

/// A consumer subscription (version 0) to `t`.
fn subscription() -> Bytes {
    Bytes::from_static(&[0, 0, 0, 0, 0, 1, 0, 1, b't', 255, 255, 255, 255])
}

struct Rig {
    group: Group,
    now: u64,
    next: u64,
    timers: HashMap<TimerKind, (u64, TimerKey)>,
    answers: HashMap<Token, Answer>,
}

impl Rig {
    fn new() -> Self {
        Self {
            group: Group::classic(settings()),
            now: 1_000,
            next: 0,
            timers: HashMap::new(),
            answers: HashMap::new(),
        }
    }

    fn send(&mut self, event: impl FnOnce(Token) -> Event) -> Token {
        self.next += 1;
        let token = Token(self.next);
        let effects = self.group.step(event(token), Millis(self.now), &Fixed);
        for effect in effects {
            match effect {
                Effect::Reply(t, a) => {
                    self.answers.insert(t, a);
                }
                Effect::Arm { key, at } => {
                    self.timers.insert(key.kind.clone(), (at.0, key));
                }
                Effect::Disarm(kind) => {
                    self.timers.remove(&kind);
                }
                Effect::Forget(_) => {}
            }
        }
        token
    }

    /// Lets `ms` pass, running the timers that fall due in the order they do.
    fn advance(&mut self, ms: u64) {
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
            self.send(|_| Event::Timer(key));
        }
        self.now = until;
    }

    fn answer(&mut self, token: Token) -> Answer {
        self.answers.remove(&token).expect("answered")
    }

    /// A classic member joins and is synced; its generation.
    fn classic_member(&mut self, name: &str) -> i32 {
        let join = self.send(|t| Event::Join(t, join_request(Some(name))));
        let Answer::Join(j) = self.answer(join) else {
            panic!("not a join")
        };
        assert!(j.error.is_none(), "{:?}", j.error);
        let (generation, leader) = (j.generation, j.leader.unwrap());
        let assignments = if leader.as_str() == name {
            let held = ByName::from([("t".to_owned(), vec![0, 1])]);
            vec![(id(name), encode_assignment(0, &held))]
        } else {
            vec![]
        };
        let sync = self.send(|t| {
            Event::Sync(
                t,
                SyncRequest {
                    member_id: id(name),
                    instance: None,
                    generation,
                    protocol_type: None,
                    protocol: None,
                    assignments,
                },
            )
        });
        let Answer::Sync(s) = self.answer(sync) else {
            panic!("not a sync")
        };
        assert!(s.error.is_none());
        generation
    }

    fn consumer(&mut self, member: &str, epoch: i32) -> crate::groups::consumer::HeartbeatAnswer {
        let t = self.send(|t| Event::ConsumerHeartbeat(t, heartbeat(member, epoch)));
        let Answer::ConsumerHeartbeat(a) = self.answer(t) else {
            panic!("not a heartbeat")
        };
        a
    }

    fn view(&mut self) -> Option<ConsumerView> {
        let t = self.send(Event::Describe);
        let Answer::Describe(d) = self.answer(t) else {
            panic!("not a description")
        };
        d.consumer
    }
}

fn join_request(member: Option<&str>) -> JoinRequest {
    JoinRequest {
        member_id: member.map(id),
        instance: None,
        protocol_type: "consumer".into(),
        protocols: vec![Protocol {
            name: "range".into(),
            metadata: subscription(),
        }],
        session_ms: 10_000,
        rebalance_ms: 30_000,
        client_id: "c".into(),
        client_host: "/127.0.0.1".into(),
        issued: true,
        id_first: false,
        rights: None,
    }
}

fn heartbeat(member: &str, epoch: i32) -> Heartbeat {
    Heartbeat {
        group: "g".into(),
        member_id: MemberId::new(member),
        version: 1,
        epoch,
        instance: None,
        rack: None,
        rebalance_ms: 60_000,
        subscribed: (epoch == 0).then(|| vec!["t".to_owned()]),
        regex: None,
        assignor: None,
        owned: (epoch == 0).then(Vec::new),
        client_id: "consumer".into(),
        client_host: "/127.0.0.1".into(),
        rights: None,
    }
}

#[test]
fn a_classic_group_of_consumers_becomes_a_consumer_group_with_its_members_in_it() {
    let mut r = Rig::new();
    let generation = r.classic_member("old");
    let joined = r.consumer("new", 0);
    assert!(joined.error.is_none(), "{:?}", joined.error);
    let view = r.view().expect("a consumer group now");
    let old = view
        .members
        .iter()
        .find(|m| m.id.as_str() == "old")
        .unwrap();
    assert!(old.classic, "the classic member stays, as a member of it");
    assert_eq!(old.epoch, generation, "at the generation it had");
    assert_eq!(old.assigned.len(), 2, "with the partitions it held");
    assert!(
        view.members
            .iter()
            .any(|m| m.id.as_str() == "new" && !m.classic)
    );
}

#[test]
fn what_a_classic_group_had_parked_is_answered_rebalance_in_progress_when_it_is_upgraded() {
    let mut r = Rig::new();
    r.classic_member("a");
    // b joins: the group is rebalancing, and b waits for a to join again.
    let b = r.send(|t| Event::Join(t, join_request(Some("b"))));
    assert!(r.answers.is_empty(), "b waits for the rebalance");
    let consumer = r.consumer("c", 0);
    assert!(consumer.error.is_none());
    let Answer::Join(j) = r.answer(b) else {
        panic!("not a join")
    };
    assert_eq!(j.error, Some(E::RebalanceInProgress));
    assert_eq!(
        j.member_id.as_ref().map(MemberId::as_str),
        Some("b"),
        "with its own id"
    );
}

#[test]
fn when_the_last_consumer_protocol_member_leaves_the_group_is_classic_again_and_its_members_rejoin()
{
    let mut r = Rig::new();
    let generation = r.classic_member("old");
    r.consumer("new", 0);
    let left = r.consumer("new", -1);
    assert!(left.error.is_none(), "{:?}", left.error);
    assert!(r.view().is_none(), "not a consumer group any more");
    // The classic member's old generation is gone: its heartbeat says so, and it rejoins.
    let hb = r.send(|t| {
        Event::Heartbeat(
            t,
            HeartbeatRequest {
                member_id: id("old"),
                instance: None,
                generation,
                rights: None,
            },
        )
    });
    let Answer::Heartbeat(e) = r.answer(hb) else {
        panic!("not a heartbeat")
    };
    assert_eq!(e, Some(E::IllegalGeneration));
    let rejoin = r.send(|t| Event::Join(t, join_request(Some("old"))));
    let Answer::Join(j) = r.answer(rejoin) else {
        panic!("not a join")
    };
    assert!(j.error.is_none(), "{:?}", j.error);
}

mod classic;
mod classic_join;
mod migration;
mod timers;
