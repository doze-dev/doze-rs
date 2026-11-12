//! The world of the liveness scenarios: well-behaved clients (consumer-protocol and classic) of three principals on four
//! topics, whose DESCRIBE rights are flipped by a seeded generator, and the checks of the group they leave.

use std::collections::{BTreeMap, BTreeSet};

use super::sim::{Pump, Rng, describe_record, may, mid, rights_of};
use super::*;
use crate::authorizer::TopicRights;
use crate::groups::consumer::protocol::ByName;
use crate::groups::consumer::{ConsumerView, TopicInfo, Topics};

pub(super) const T: [&str; 4] = ["t0", "t1", "t2", "t3"];
const PRINCIPALS: [&str; 3] = ["alice", "bob", "carol"];

#[derive(Clone)]
pub(super) enum Kind {
    Cons { regex: Option<String> },
    Classic { coop: bool },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) enum Phase {
    NeedJoin,
    NeedSync(i32),
    Active(i32),
}

pub(super) struct Client {
    pub(super) name: String,
    pub(super) who: usize,
    pub(super) subs: Vec<String>,
    pub(super) kind: Kind,
    /// The consumer protocol: the member epoch and the partitions it owns.
    pub(super) epoch: i32,
    pub(super) owned: Vec<(Uuid, Vec<i32>)>,
    /// The classic protocol: where the client is in its loop, and what it holds.
    pub(super) phase: Phase,
    pub(super) held: ByName,
}

impl Client {
    fn new(name: &str, who: usize, subs: &[&str], kind: Kind) -> Self {
        Self {
            name: name.into(),
            who,
            subs: subs.iter().map(|s| (*s).to_owned()).collect(),
            kind,
            epoch: 0,
            owned: Vec::new(),
            phase: Phase::NeedJoin,
            held: ByName::new(),
        }
    }
}

pub(super) struct Sim {
    pub(super) pump: Pump,
    pub(super) topics: BTreeMap<String, (Uuid, i32)>,
    pub(super) clients: Vec<Client>,
    pub(super) rights: Vec<Option<TopicRights>>,
    pub(super) denied_bob: BTreeSet<usize>,
    pub(super) granted_carol: BTreeSet<usize>,
    pub(super) rng: Rng,
    /// How many times a classic client was answered REBALANCE_IN_PROGRESS to a heartbeat.
    pub(super) rebalance_signals: usize,
    pub(super) errors: BTreeMap<String, usize>,
    pub(super) overlaps: Vec<String>,
}

impl Sim {
    pub(super) fn new(seed: u64, assign_interval_ms: u64) -> Self {
        let mut sim = Self {
            pump: Pump::new(assign_interval_ms),
            topics: BTreeMap::new(),
            clients: Vec::new(),
            rights: Vec::new(),
            denied_bob: BTreeSet::new(),
            granted_carol: BTreeSet::new(),
            rng: Rng::from_seed(seed),
            rebalance_signals: 0,
            errors: BTreeMap::new(),
            overlaps: Vec::new(),
        };
        for (i, t) in T.iter().enumerate() {
            let partitions = 1 + i32::try_from(sim.rng.below(3)).unwrap();
            sim.topics.insert(
                (*t).to_owned(),
                (Uuid::from_u128(10 + i as u128), partitions),
            );
        }
        sim.set_rights();
        sim
    }

    pub(super) fn snapshot(&self) -> Topics {
        Topics::new(self.topics.iter().map(|(n, (id, p))| TopicInfo {
            name: n.clone(),
            id: *id,
            partitions: *p,
        }))
    }

    pub(super) fn send(&mut self, event: impl FnOnce(Token) -> Event) -> u64 {
        let topics = self.snapshot();
        self.pump.send(&topics, event)
    }

    pub(super) fn answer(&mut self, token: u64) -> Option<Answer> {
        self.pump.take(token)
    }

    pub(super) fn advance(&mut self, ms: u64) {
        let topics = self.snapshot();
        self.pump.advance(&topics, ms);
    }

    /// The rights the principals have under the ACLs in force: alice may DESCRIBE all; bob all but the topics denied to
    /// him; carol only the topics granted to her.
    pub(super) fn set_rights(&mut self) {
        let mut recs = vec![
            describe_record("", 4, "bob", true),
            describe_record("", 4, "alice", true),
        ];
        recs.extend(
            self.denied_bob
                .iter()
                .map(|i| describe_record(T[*i], 3, "bob", false)),
        );
        recs.extend(
            self.granted_carol
                .iter()
                .map(|i| describe_record(T[*i], 3, "carol", true)),
        );
        self.rights = rights_of(recs, false, &[], &PRINCIPALS);
    }

    pub(super) fn add_cons(&mut self, name: &str, who: usize, subs: &[&str], regex: Option<&str>) {
        let kind = Kind::Cons {
            regex: regex.map(str::to_owned),
        };
        self.clients.push(Client::new(name, who, subs, kind));
    }

    pub(super) fn add_classic(&mut self, name: &str, who: usize, subs: &[&str], coop: bool) {
        let kind = Kind::Classic { coop };
        self.clients.push(Client::new(name, who, subs, kind));
    }

    pub(super) fn view(&mut self) -> ConsumerView {
        let t = self.send(Event::Describe);
        let Some(Answer::Describe(d)) = self.answer(t) else {
            panic!("describe")
        };
        d.consumer.expect("a consumer group")
    }

    /// Changes the ACLs of bob and carol at random.
    pub(super) fn flip(&mut self) {
        let t = self.rng.below(4);
        match self.rng.below(4) {
            0 => toggle(&mut self.denied_bob, t),
            1 => toggle(&mut self.granted_carol, t),
            2 => self.denied_bob.clear(),
            _ => {
                self.denied_bob.insert(t);
            }
        }
        self.set_rights();
    }

    /// The invariant of this instant: no partition is owned by two members.
    pub(super) fn check(&mut self, label: &str) -> ConsumerView {
        let v = self.view();
        let mut seen: BTreeMap<(Uuid, i32), String> = BTreeMap::new();
        for m in &v.members {
            for (t, p) in m.assigned.iter() {
                if let Some(prev) = seen.insert((*t, *p), m.id.as_str().to_owned()) {
                    self.overlaps
                        .push(format!("{label}: {t:?}/{p} held by {prev} and {}", m.id));
                }
            }
        }
        v
    }

    /// What members hold or show that their principals' CURRENT rights do not let them DESCRIBE.
    pub(super) fn hidden_held(&self, v: &ConsumerView) -> Vec<String> {
        let mut bad = Vec::new();
        for m in &v.members {
            let c = self
                .clients
                .iter()
                .find(|c| c.name == m.id.as_str())
                .unwrap();
            for (t, p) in m.assigned.iter().chain(m.target.iter()) {
                let name = self.name_of(t);
                if !may(&self.rights[c.who], &name) {
                    bad.push(format!("{} holds {name}/{p}", m.id));
                }
            }
            for n in m.subscribed.iter().filter(|n| !may(&self.rights[c.who], n)) {
                bad.push(format!("{} shows hidden name {n}", m.id));
            }
        }
        bad
    }

    fn name_of(&self, id: &Uuid) -> String {
        self.topics
            .iter()
            .find(|(_, (i, _))| i == id)
            .map(|(n, _)| n.clone())
            .unwrap_or_default()
    }

    /// Partitions of topics some member may have and wants, that no member holds.
    pub(super) fn unowned(&self, v: &ConsumerView) -> Vec<String> {
        let mut out = Vec::new();
        for (n, (id, parts)) in &self.topics {
            if !self.clients.iter().any(|c| self.wants(c, n)) {
                continue;
            }
            for p in 0..*parts {
                if !v.members.iter().any(|m| m.assigned.contains(&(*id, p))) {
                    out.push(format!("{n}/{p}"));
                }
            }
        }
        out
    }

    fn wants(&self, c: &Client, topic: &str) -> bool {
        if !may(&self.rights[c.who], topic) {
            return false;
        }
        let named = c.subs.iter().any(|s| s == topic);
        match &c.kind {
            Kind::Cons { regex } => {
                named || regex.as_deref().is_some_and(|r| regex_matches(r, topic))
            }
            Kind::Classic { .. } => named,
        }
    }

    /// The `mid` of a client (its member id is its name).
    pub(super) fn member(&self, i: usize) -> MemberId {
        mid(&self.clients[i].name)
    }
}

fn toggle(set: &mut BTreeSet<usize>, t: usize) {
    if !set.remove(&t) {
        set.insert(t);
    }
}

fn regex_matches(r: &str, name: &str) -> bool {
    match r {
        "t.*" => name.starts_with('t'),
        "t[0-1]" => name == "t0" || name == "t1",
        "t[2-3]" => name == "t2" || name == "t3",
        _ => false,
    }
}
