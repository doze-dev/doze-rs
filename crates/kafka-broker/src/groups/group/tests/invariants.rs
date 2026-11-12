//! An invariant generator for a consumer group and DESCRIBE rights (SEC-210, SEC-230 to SEC-236), independent of the
//! per-source tests: random single requests (consumer-protocol and classic, static instances, member ids another
//! principal uses, a join parked in the group), with ACL changes, topic changes, expiries and admin leaves between any
//! two of them. After EVERY event no member shows (its target, its effective names) a topic its LAST PRESENTED rights
//! do not let it DESCRIBE, and nothing a member was told (the assignment of a ConsumerGroupHeartbeat, the bytes of a
//! classic SyncGroup) names one either.
//!
//! The checker is lenient in one case: a member with a classic JoinGroup parked in the group, or whose JoinGroup was
//! answered REBALANCE_IN_PROGRESS (the rights of that request are taken but its subscription is not yet), is exempt
//! from the check of what the group shows of it until a join of it succeeds or another request of it is answered.
//! The generator `step`s (in `invariants_steps`) are seeded and reproducible.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::invariants_steps::T;
use super::sim::{Pump, Rng, describe_record, may, rights_of};
use super::*;
use crate::authorizer::TopicRights;
use crate::groups::consumer::{TopicInfo, Topics};

pub(super) const P: [&str; 4] = ["u0", "u1", "u2", "u3"];

/// The ACLs of the model: principal, topic name or prefix, prefixed, allow.
#[derive(Clone, Default)]
pub(super) struct Acls(pub(super) Vec<(usize, &'static str, bool, bool)>);

pub(super) struct World {
    pub(super) pump: Pump,
    pub(super) topics: BTreeMap<String, (Uuid, i32)>,
    pub(super) acls: Acls,
    pub(super) super_user: Option<usize>,
    pub(super) default_allow: bool,
    pub(super) rights: Vec<Option<TopicRights>>,
    /// Per member id: the rights it last presented.
    pub(super) last: BTreeMap<String, Option<TopicRights>>,
    /// What each client believes: its consumer-protocol epoch and ownership, its classic generation and holdings.
    pub(super) epoch: BTreeMap<String, i32>,
    pub(super) owned: BTreeMap<String, Vec<(Uuid, Vec<i32>)>>,
    pub(super) gn: BTreeMap<String, i32>,
    pub(super) held: BTreeMap<String, BTreeMap<String, Vec<i32>>>,
    pub(super) rng: Rng,
    pub(super) violations: Vec<String>,
    pub(super) checks: usize,
    pub(super) events: usize,
    pub(super) ids: u128,
    pub(super) trace: Vec<String>,
    /// Classic joins sent whose answer is not seen yet: token -> (name, rights of the request).
    pub(super) join_tokens: HashMap<u64, (String, Option<TopicRights>)>,
    /// Members whose last JoinGroup was answered REBALANCE_IN_PROGRESS.
    pub(super) stale_ok: BTreeSet<String>,
}

pub(super) fn rights_under(
    acls: &Acls,
    super_user: Option<usize>,
    default_allow: bool,
) -> Vec<Option<TopicRights>> {
    let records = acls
        .0
        .iter()
        .map(|(p, name, prefixed, allow)| {
            describe_record(name, if *prefixed { 4 } else { 3 }, P[*p], *allow)
        })
        .collect();
    let supers: Vec<&str> = super_user.map(|u| P[u]).into_iter().collect();
    rights_of(records, default_allow, &supers, &P)
}

impl World {
    pub(super) fn new(seed: u64) -> Self {
        let mut rng = Rng::from_seed(seed);
        let assign = [0_u64, 1_000, 3_000][rng.below(3)];
        let super_user = if rng.pct(30) {
            Some(rng.below(4))
        } else {
            None
        };
        let default_allow = rng.pct(25);
        let mut w = Self {
            pump: Pump::new(assign),
            topics: BTreeMap::new(),
            acls: Acls::default(),
            super_user,
            default_allow,
            rights: Vec::new(),
            last: BTreeMap::new(),
            epoch: BTreeMap::new(),
            owned: BTreeMap::new(),
            gn: BTreeMap::new(),
            held: BTreeMap::new(),
            rng,
            violations: Vec::new(),
            checks: 0,
            events: 0,
            ids: 50,
            trace: Vec::new(),
            join_tokens: HashMap::new(),
            stale_ok: BTreeSet::new(),
        };
        for t in &T[..3] {
            w.make(t);
        }
        w.rights = rights_under(&w.acls, w.super_user, w.default_allow);
        w
    }

    pub(super) fn make(&mut self, t: &str) {
        self.ids += 1;
        let partitions = 1 + i32::try_from(self.rng.below(3)).unwrap();
        self.topics
            .insert(t.to_owned(), (Uuid::from_u128(self.ids), partitions));
    }

    pub(super) fn name_of(&self, id: &Uuid) -> String {
        self.topics
            .iter()
            .find(|(_, (i, _))| i == id)
            .map(|(n, _)| n.clone())
            .unwrap_or_default()
    }

    fn snapshot(&self) -> Topics {
        Topics::new(self.topics.iter().map(|(n, (id, p))| TopicInfo {
            name: n.clone(),
            id: *id,
            partitions: *p,
        }))
    }

    /// Sends an event; the token. The answers of parked joins that came with it are taken in.
    pub(super) fn send(&mut self, event: impl FnOnce(Token) -> Event) -> u64 {
        self.events += 1;
        let topics = self.snapshot();
        let token = self.pump.send(&topics, event);
        self.reap_joins();
        token
    }

    pub(super) fn take(&mut self, token: u64) -> Option<Answer> {
        self.pump.take(token)
    }

    /// Sends a classic JoinGroup of the member `name`, whose answer may come later.
    pub(super) fn send_join(&mut self, name: &str, rights: &Option<TopicRights>, req: JoinRequest) {
        let t = self.send(|t| Event::Join(t, req));
        self.join_tokens
            .insert(t, (name.to_owned(), rights.clone()));
        self.reap_joins();
    }

    /// A classic join is answered when it is, maybe long after it was sent; what the answer says of the member's rights
    /// is learnt then.
    fn reap_joins(&mut self) {
        let tokens: Vec<u64> = self.join_tokens.keys().copied().collect();
        for t in tokens {
            let Some(Answer::Join(j)) = self.pump.take(t) else {
                continue;
            };
            let Some((name, rights)) = self.join_tokens.remove(&t) else {
                continue;
            };
            let id = j
                .member_id
                .as_ref()
                .map_or(name.clone(), |m| m.as_str().to_owned());
            if j.error == Some(E::RebalanceInProgress) {
                self.last.insert(id.clone(), rights);
                self.stale_ok.insert(id);
            } else if j.error.is_none() {
                self.last.insert(id.clone(), rights);
                self.stale_ok.remove(&id);
                self.gn.insert(id, j.generation);
                self.gn.insert(name, j.generation);
            }
        }
    }

    pub(super) fn advance(&mut self, ms: u64) {
        let until = self.pump.now + ms;
        let topics = self.snapshot();
        while self.pump.fire_next(&topics, until) {
            self.trace.push("timer".into());
            self.reap_joins();
            self.check_all("timer");
        }
        self.pump.now = until;
    }

    pub(super) fn bad(&mut self, what: String) {
        if self.violations.is_empty() {
            let tail: Vec<String> = self.trace.iter().rev().take(40).rev().cloned().collect();
            self.violations.push(format!(
                "{what}\n      trace: {}",
                tail.join("\n             ")
            ));
        }
    }

    /// Every member of the group, against the rights it last presented.
    pub(super) fn check_all(&mut self, label: &str) {
        let t = self.send(Event::Describe);
        let Some(Answer::Describe(d)) = self.take(t) else {
            return;
        };
        let Some(v) = d.consumer else { return };
        for m in &v.members {
            self.checks += 1;
            let id = m.id.as_str();
            let exempt =
                self.stale_ok.contains(id) || self.join_tokens.values().any(|(n, _)| n == id);
            if exempt {
                continue;
            }
            let rights = self.last.get(id).cloned().unwrap_or(None);
            let hidden: Vec<String> = m
                .target
                .iter()
                .map(|(t, p)| (self.name_of(t), *p))
                .filter(|(n, _)| !n.is_empty() && !may(&rights, n))
                .map(|(n, p)| format!("target holds {n}/{p}"))
                .chain(
                    m.subscribed
                        .iter()
                        .filter(|n| !may(&rights, n))
                        .map(|n| format!("shows hidden name {n}")),
                )
                .collect();
            if let Some(first) = hidden.first() {
                self.bad(format!(
                    "{label} #{}: {id} {first} hidden from its last rights",
                    self.events
                ));
            }
        }
    }

    /// What a consumer-protocol client learns from an answer: its epoch, and the partitions it was told.
    pub(super) fn learn_848(&mut self, who: &str, rights: &Option<TopicRights>, a: Answer) {
        let Answer::ConsumerHeartbeat(h) = a else {
            return;
        };
        if h.error.is_some() {
            self.epoch.insert(who.into(), 0);
            self.owned.insert(who.into(), Vec::new());
            return;
        }
        self.epoch.insert(who.into(), h.epoch);
        let Some(assignment) = &h.assignment else {
            return;
        };
        let mut by: BTreeMap<Uuid, Vec<i32>> = BTreeMap::new();
        for (t, p) in assignment {
            by.entry(*t).or_default().push(*p);
            let n = self.name_of(t);
            if !n.is_empty() && !may(rights, &n) {
                self.bad(format!(
                    "#{}: {who} was TOLD {n}/{p} (hidden from the rights of that very request)",
                    self.events
                ));
            }
        }
        self.owned.insert(who.into(), by.into_iter().collect());
    }
}

/// Seeds of the generator and steps of each. The audit ran 7,000 seeds of 400 steps (52 s) with no violation; 1,000 take about 8 s of a debug
/// build.
const SEEDS: u64 = 1000;
const STEPS: usize = 400;

#[test]
fn no_request_shows_or_tells_a_member_a_topic_its_last_rights_do_not_let_it_describe() {
    let mut bad: Vec<String> = Vec::new();
    let (mut checks, mut events) = (0, 0);
    for seed in 1..=SEEDS {
        let mut w = World::new(seed);
        for _ in 0..STEPS {
            w.step();
        }
        checks += w.checks;
        events += w.events;
        bad.extend(w.violations.iter().map(|v| format!("seed {seed}: {v}")));
    }
    assert!(
        checks > 100_000 && events > 100_000,
        "the run did something: {checks} checks, {events} events"
    );
    assert!(
        bad.is_empty(),
        "{} violations: {:#?}",
        bad.len(),
        &bad[..bad.len().min(3)]
    );
}
