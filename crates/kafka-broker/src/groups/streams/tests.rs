//! The streams group's rules, each held by a test that fails without it (docs/port/behaviours/streams-groups.md). The clock
//! is the test's: every step is handed the time, and timers are run by hand.

use std::collections::BTreeMap;

use kafka_wire::ResponseError as E;
use kafka_wire::Uuid;
use kafka_wire::ids::{InstanceId, MemberId};

use super::topology::{Subtopology, TopicInfo, Topology};
use super::{StreamsAnswer, StreamsGroup, StreamsHeartbeat, StreamsTiming, Task, Tasks};
use crate::groups::consumer::{TopicInfo as Topic, Topics};
use crate::groups::machine::{Answer, Effect, Millis, TimerKey, TimerKind, Token};

mod assignor;
mod configure;
mod epochs;
mod fences;
mod members;
mod reconciling;
mod restart;

pub(super) fn timing() -> StreamsTiming {
    StreamsTiming {
        session_ms: 45_000,
        heartbeat_ms: 5_000,
        initial_delay_ms: 3_000,
        assign_interval_ms: 1_000,
        task_offset_ms: 60_000,
        acceptable_lag: 10_000,
        standbys: 0,
        tags: Vec::new(),
        max_size: usize::MAX,
    }
}

pub(super) fn topics(list: &[(&str, i32)]) -> Topics {
    Topics::new(list.iter().enumerate().map(|(i, (name, partitions))| Topic {
        name: (*name).to_owned(),
        id: Uuid::from_u128(u128::try_from(i).unwrap() + 1),
        partitions: *partitions,
    }))
}

pub(super) fn internal(name: &str, partitions: i32) -> TopicInfo {
    TopicInfo {
        name: name.to_owned(),
        partitions,
        replication: 0,
        configs: Vec::new(),
    }
}

/// The word count: subtopology 0 reads `in` and writes `app-rep`; subtopology 1 reads `app-rep` and keeps a changelog.
pub(super) fn word_count() -> Topology {
    let mut subtopologies = BTreeMap::new();
    subtopologies.insert(
        "0".to_owned(),
        Subtopology {
            sources: vec!["in".to_owned()],
            repartition_sinks: vec!["app-rep".to_owned()],
            ..Subtopology::default()
        },
    );
    subtopologies.insert(
        "1".to_owned(),
        Subtopology {
            repartition_sources: vec![internal("app-rep", 0)],
            changelogs: vec![internal("app-counts-changelog", 0)],
            ..Subtopology::default()
        },
    );
    Topology {
        epoch: 0,
        subtopologies,
    }
}

/// The topics the word count runs with once its internal topics are made.
pub(super) fn ready_topics(partitions: i32) -> Topics {
    topics(&[("in", partitions), ("app-rep", partitions), ("app-counts-changelog", partitions)])
}

pub(super) fn id(s: &str) -> MemberId {
    MemberId::new(s).unwrap()
}

pub(super) fn tasks(active: &[(&str, i32)], standby: &[(&str, i32)]) -> Tasks {
    let set = |l: &[(&str, i32)]| l.iter().map(|(s, p)| ((*s).to_owned(), *p)).collect();
    Tasks {
        active: set(active),
        standby: set(standby),
        warmup: std::collections::BTreeSet::new(),
    }
}

/// A join: epoch 0, the topology, empty task lists, a process of its own.
pub(super) fn join(member: &str) -> StreamsHeartbeat {
    StreamsHeartbeat {
        group: "app".to_owned(),
        member_id: id(member),
        epoch: 0,
        version: 1,
        endpoint_epoch: -1,
        instance: None,
        rack: None,
        rebalance_ms: 300_000,
        topology: Some(word_count()),
        owned: Some(Tasks::default()),
        process: Some(format!("process-{member}")),
        endpoint: None,
        client_tags: Some(BTreeMap::new()),
        offsets: Some(Vec::new()),
        end_offsets: Some(Vec::new()),
        shutdown: false,
        client_id: format!("client-{member}"),
        client_host: "/127.0.0.1".to_owned(),
        timing: timing(),
    }
}

/// A heartbeat that keeps on at `epoch`, owning `owned` (or not saying).
pub(super) fn beat(member: &str, epoch: i32, owned: Option<Tasks>) -> StreamsHeartbeat {
    StreamsHeartbeat {
        epoch,
        topology: None,
        rebalance_ms: -1,
        owned,
        process: None,
        client_tags: None,
        offsets: None,
        end_offsets: None,
        ..join(member)
    }
}

/// A group, the time, and the timers its effects armed.
pub(super) struct World {
    pub(super) group: StreamsGroup,
    pub(super) now: Millis,
    pub(super) topics: Topics,
    pub(super) timers: std::collections::HashMap<TimerKind, (u64, Millis)>,
    tokens: u64,
}

impl World {
    pub(super) fn new(topics: Topics) -> Self {
        Self {
            group: StreamsGroup::new(),
            now: Millis(1_000),
            topics,
            timers: std::collections::HashMap::new(),
            tokens: 0,
        }
    }

    fn apply(&mut self, effects: Vec<Effect>) -> Option<StreamsAnswer> {
        let mut answer = None;
        for e in effects {
            match e {
                Effect::Arm { key, at } => {
                    self.timers.insert(key.kind, (key.generation, at));
                }
                Effect::Disarm(kind) => {
                    self.timers.remove(&kind);
                }
                Effect::Reply(_, Answer::StreamsHeartbeat(a)) => answer = Some(a),
                _ => {}
            }
        }
        answer
    }

    pub(super) fn send(&mut self, hb: &StreamsHeartbeat) -> StreamsAnswer {
        self.tokens += 1;
        let mut out = Vec::new();
        self.group.heartbeat(Token(self.tokens), hb, &self.topics, self.now, &mut out);
        self.apply(out).expect("an answer")
    }

    /// Moves the clock and runs every timer that is due, in time order.
    pub(super) fn advance(&mut self, ms: u64) {
        self.now = self.now.plus(ms);
        loop {
            let due = self
                .timers
                .iter()
                .filter(|(_, (_, at))| *at <= self.now)
                .min_by_key(|(_, (_, at))| *at)
                .map(|(k, (g, _))| (k.clone(), *g));
            let Some((kind, generation)) = due else { break };
            self.timers.remove(&kind);
            let mut out = Vec::new();
            self.group.timer(&TimerKey { kind, generation }, self.now, &mut out);
            self.apply(out);
        }
    }

    /// A member joins and keeps on until it holds its target and the group is stable; its last answer.
    pub(super) fn settle(&mut self, members: &[&str]) -> BTreeMap<String, StreamsAnswer> {
        let mut last: BTreeMap<String, StreamsAnswer> = BTreeMap::new();
        for _ in 0..20 {
            for m in members {
                let hb = match last.get(*m) {
                    None => join(m),
                    Some(a) => beat(m, a.epoch, Some(self.held(m))),
                };
                let a = self.send(&hb);
                assert!(a.error.is_none(), "{m}: {:?}", a.error);
                last.insert((*m).to_owned(), a);
            }
            if self.group.state() == "Stable" {
                break;
            }
            self.advance(1_000);
        }
        last
    }

    /// What the group says the member holds.
    pub(super) fn held(&self, member: &str) -> Tasks {
        self.group.members.get(&id(member)).map(|m| m.held.tasks.clone()).unwrap_or_default()
    }
}

pub(super) fn active_count(t: &Tasks) -> usize {
    t.active.len()
}

pub(super) fn task(sub: &str, p: i32) -> Task {
    (sub.to_owned(), p)
}

pub(super) fn instance(s: &str) -> Option<InstanceId> {
    Some(InstanceId::new(s))
}

pub(super) fn refused(a: &StreamsAnswer) -> (E, &str) {
    let (e, w) = a.error.as_ref().expect("refused");
    (*e, w.as_str())
}
