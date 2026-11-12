//! A group is driven by the same seeded sequences of members joining, leaving, going silent and being told of topics,
//! and what it answers, what it arms and what it says of itself is hashed. The hash of the whole run is a constant,
//! worked out on the machine as it was before any note was added to it (slice 9f): a note is an extra effect, never a
//! different transition, and this is the test that says so.

use std::collections::{BTreeMap, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};

use super::sim::{Fixed, Rng, made_ids, subscription_of};
use super::*;
use crate::groups::consumer::protocol::{ByName, encode_assignment};
use crate::groups::consumer::{Heartbeat, TopicInfo, Topics};
use crate::groups::machine::{
    Answer, Effect, Event, HeartbeatRequest, JoinRequest, LeaveTarget, Millis, Protocol,
    SyncRequest, TimerKey, TimerKind, Token,
};
use kafka_wire::ids::{InstanceId, MemberId};

const NAMES: [&str; 5] = ["a", "b", "c", "d", "e"];
const TOPICS: [&str; 3] = ["t0", "t1", "t2"];

/// What a run of one seed leaves: the hash of everything the group did that is not a note.
struct Run {
    group: Group,
    rng: Rng,
    now: u64,
    next: u64,
    timers: HashMap<TimerKind, (u64, TimerKey)>,
    topics: BTreeMap<String, (Uuid, i32)>,
    /// The generation each classic member last joined at, and the epoch each consumer member last heard.
    generation: HashMap<usize, i32>,
    epoch: HashMap<usize, i32>,
    owned: HashMap<usize, Vec<(Uuid, Vec<i32>)>>,
    hash: DefaultHasher,
    /// How many notes the group said (none before slice 9f).
    notes: usize,
    /// Consumer-protocol members come and go too (the group changes kind with them); not in half the runs, which stay classic.
    mixed: bool,
    /// The JoinGroups still waiting for their answer, by token, and whose they are.
    joining: HashMap<u64, usize>,
    /// What the run did, by kind of answer: the sequences must reach the cases that matter.
    tally: BTreeMap<String, usize>,
}

/// What an effect is, in a word: the kind of answer and whether it was a yes.
fn label(effect: &Effect) -> String {
    let error =
        |e: &Option<kafka_wire::ResponseError>| e.map_or("ok".to_owned(), |e| format!("{e:?}"));
    match effect {
        Effect::Reply(_, Answer::Join(j)) => format!("join:{}", error(&j.error)),
        Effect::Reply(_, Answer::Sync(s)) => format!("sync:{}", error(&s.error)),
        Effect::Reply(_, Answer::Heartbeat(e)) => format!("beat:{}", error(e)),
        Effect::Reply(_, Answer::ConsumerHeartbeat(h)) => {
            format!(
                "consumer:{}",
                h.error
                    .as_ref()
                    .map_or("ok".to_owned(), |(e, _)| format!("{e:?}"))
            )
        }
        Effect::Reply(_, Answer::Leave(_)) => "leave".to_owned(),
        Effect::Reply(_, Answer::Describe(d)) => {
            format!("describe:{:?}:{}", d.state, d.consumer.is_some())
        }
        Effect::Reply(..) => "reply".to_owned(),
        Effect::Arm { key, .. } => format!(
            "arm:{:?}",
            std::mem::discriminant(&key.kind) == std::mem::discriminant(&TimerKind::Rebalance)
        ),
        Effect::Disarm(_) => "disarm".to_owned(),
        Effect::Forget(_) => "forget".to_owned(),
        #[allow(unreachable_patterns)]
        _ => "other".to_owned(),
    }
}

fn settings(initial_delay_ms: u64, assign_interval_ms: u64) -> Settings {
    Settings {
        initial_delay_ms,
        consumer: consumer::Config {
            session_ms: 45_000,
            assign_interval_ms,
        },
        ids: made_ids,
    }
}

fn is_note(effect: &Effect) -> bool {
    format!("{effect:?}").starts_with("Note")
}

impl Run {
    fn new(seed: u64) -> Self {
        let mut rng = Rng::from_seed(seed);
        let delay = [0, 0, 300][rng.below(3)];
        let interval = [0, 0, 5_000][rng.below(3)];
        let mut topics = BTreeMap::new();
        for (i, t) in TOPICS.iter().enumerate() {
            topics.insert(
                (*t).to_owned(),
                (
                    Uuid::from_u128(u128::try_from(i).unwrap() + 1),
                    1 + i32::try_from(rng.below(3)).unwrap(),
                ),
            );
        }
        Self {
            group: Group::classic(settings(delay, interval)),
            rng,
            now: 1_000,
            next: 0,
            timers: HashMap::new(),
            topics,
            generation: HashMap::new(),
            epoch: HashMap::new(),
            owned: HashMap::new(),
            hash: DefaultHasher::new(),
            notes: 0,
            tally: BTreeMap::new(),
            mixed: seed % 2 == 1,
            joining: HashMap::new(),
        }
    }

    fn table(&self) -> Topics {
        Topics::new(
            self.topics
                .iter()
                .map(|(name, (id, partitions))| TopicInfo {
                    name: name.clone(),
                    id: *id,
                    partitions: *partitions,
                }),
        )
    }

    fn send(&mut self, event: impl FnOnce(Token) -> Event) -> Vec<Effect> {
        self.next += 1;
        let source = Fixed(self.table());
        let effects = self
            .group
            .step(event(Token(self.next)), Millis(self.now), &source);
        let mut seen = Vec::new();
        for effect in effects {
            if is_note(&effect) {
                self.notes += 1;
                continue;
            }
            format!("{effect:?}").hash(&mut self.hash);
            if let Effect::Reply(token, Answer::Join(j)) = &effect
                && j.error.is_none()
                && let Some(i) = self.joining.remove(&token.0)
            {
                self.generation.insert(i, j.generation);
            }
            *self.tally.entry(label(&effect)).or_default() += 1;
            match &effect {
                Effect::Arm { key, at } => {
                    self.timers.insert(key.kind.clone(), (at.0, key.clone()));
                }
                Effect::Disarm(kind) => {
                    self.timers.remove(kind);
                }
                _ => {}
            }
            seen.push(effect);
        }
        seen
    }

    fn name(&mut self) -> usize {
        self.rng.below(NAMES.len())
    }

    fn classic_join(&mut self, i: usize, fresh: bool) {
        let alt = self.rng.pct(15);
        let subs: Vec<String> = TOPICS
            .iter()
            .take(1 + self.rng.below(TOPICS.len()))
            .map(|t| (*t).to_owned())
            .collect();
        let protocols = vec![Protocol {
            name: if alt { "roundrobin" } else { "range" }.into(),
            metadata: subscription_of(1, &subs, &ByName::new()),
        }];
        let statik = self.rng.pct(15);
        let req = JoinRequest {
            member_id: (!fresh).then(|| MemberId::new(&format!("m-{}", NAMES[i])).unwrap()),
            instance: statik.then(|| InstanceId::new(&format!("i-{}", NAMES[i]))),
            protocol_type: "consumer".into(),
            protocols,
            session_ms: [6_000, 10_000, 30_000][self.rng.below(3)],
            rebalance_ms: [5_000, 30_000, 60_000][self.rng.below(3)],
            client_id: format!("client-{}", NAMES[i]),
            client_host: "/127.0.0.1".into(),
            issued: true,
            id_first: false,
            rights: None,
        };
        self.joining.insert(self.next + 1, i);
        self.send(|t| Event::Join(t, req));
    }

    /// Members join, and the leader gives out the assignment, with nobody late: the group is Stable if it can be.
    fn round(&mut self) {
        let members: Vec<usize> = (0..NAMES.len()).filter(|_| self.rng.pct(60)).collect();
        for &i in &members {
            self.classic_join(i, false);
        }
        self.advance(400);
        for &i in &members {
            self.classic_sync(i);
        }
    }

    fn classic_sync(&mut self, i: usize) {
        let generation = self
            .generation
            .get(&i)
            .copied()
            .unwrap_or(0)
            .saturating_add(i32::from(self.rng.pct(5)));
        let held = ByName::from([("t0".to_owned(), vec![0])]);
        let assignments = NAMES
            .iter()
            .enumerate()
            .filter(|_| self.rng.pct(60))
            .map(|(k, _)| {
                (
                    MemberId::new(&format!("m-{}", NAMES[k])).unwrap(),
                    encode_assignment(0, &held),
                )
            })
            .collect();
        let req = SyncRequest {
            member_id: MemberId::new(&format!("m-{}", NAMES[i])).unwrap(),
            instance: None,
            generation,
            protocol_type: None,
            protocol: None,
            assignments,
        };
        self.send(|t| Event::Sync(t, req));
    }

    fn classic_beat(&mut self, i: usize) {
        let req = HeartbeatRequest {
            member_id: MemberId::new(&format!("m-{}", NAMES[i])).unwrap(),
            instance: None,
            generation: self.generation.get(&i).copied().unwrap_or(0),
            rights: None,
        };
        self.send(|t| Event::Heartbeat(t, req));
    }

    fn classic_leave(&mut self, i: usize) {
        let by_instance = self.rng.pct(20);
        let target = LeaveTarget {
            member_id: (!by_instance).then(|| MemberId::new(&format!("m-{}", NAMES[i])).unwrap()),
            instance: by_instance.then(|| InstanceId::new(&format!("i-{}", NAMES[i]))),
        };
        self.send(|t| Event::Leave(t, vec![target]));
    }

    fn consumer_beat(&mut self, i: usize) {
        let known = self.epoch.get(&i).copied().unwrap_or(0);
        let roll = self.rng.below(100);
        let epoch = match roll {
            0..=14 => 0,
            15..=24 => -1,
            _ => known,
        };
        let subscribed = (epoch == 0 || self.rng.pct(10)).then(|| {
            TOPICS
                .iter()
                .take(1 + self.rng.below(TOPICS.len()))
                .map(|t| (*t).to_owned())
                .collect()
        });
        let owned = (epoch == 0 || self.rng.pct(60)).then(|| {
            if epoch == 0 {
                Vec::new()
            } else {
                self.owned.get(&i).cloned().unwrap_or_default()
            }
        });
        let req = Heartbeat {
            group: "g".into(),
            member_id: MemberId::new(&format!("c-{}", NAMES[i])),
            version: 1,
            epoch,
            instance: None,
            rack: None,
            rebalance_ms: [5_000, 30_000, 60_000][self.rng.below(3)],
            subscribed,
            regex: None,
            assignor: None,
            owned,
            client_id: "consumer".into(),
            client_host: "/127.0.0.1".into(),
            rights: None,
        };
        for effect in self.send(|t| Event::ConsumerHeartbeat(t, req)) {
            if let Effect::Reply(_, Answer::ConsumerHeartbeat(h)) = effect {
                if h.error.is_some() {
                    self.epoch.insert(i, 0);
                } else {
                    self.epoch.insert(i, h.epoch);
                    if let Some(parts) = h.assignment {
                        let mut by: BTreeMap<Uuid, Vec<i32>> = BTreeMap::new();
                        for (t, p) in parts {
                            by.entry(t).or_default().push(p);
                        }
                        self.owned.insert(i, by.into_iter().collect());
                    }
                }
            }
        }
    }

    fn advance(&mut self, ms: u64) {
        let until = self.now + ms;
        loop {
            let due = self
                .timers
                .values()
                .filter(|(at, _)| *at <= until)
                .min_by_key(|(at, key)| (*at, key.generation, format!("{:?}", key.kind)))
                .map(|(at, key)| (*at, key.clone()));
            let Some((at, key)) = due else { break };
            self.timers.remove(&key.kind);
            self.now = at;
            self.send(|_| Event::Timer(key));
        }
        self.now = until;
    }

    fn topics_change(&mut self) {
        let t = TOPICS[self.rng.below(TOPICS.len())];
        if let Some((_, partitions)) = self.topics.get_mut(t) {
            *partitions += 1;
        }
        self.send(|_| Event::TopicsChanged);
    }

    fn step(&mut self) {
        let i = self.name();
        match self.rng.below(100) {
            0..=19 => self.classic_join(i, false),
            20..=22 => self.classic_join(i, true),
            23..=34 => self.classic_sync(i),
            35..=44 => self.classic_beat(i),
            45..=53 => self.classic_leave(i),
            54..=68 => {
                let ms = [0, 500, 3_000, 12_000, 40_000][self.rng.below(5)];
                self.advance(ms);
            }
            69..=84 if self.mixed => self.consumer_beat(i),
            69..=84 => self.classic_join(i, false),
            85..=89 => self.topics_change(),
            90..=94 => self.round(),
            _ => {
                self.send(Event::Describe);
            }
        }
        // Time passes between requests.
        self.now += 1 + u64::try_from(self.rng.below(200)).unwrap();
        if self.next.is_multiple_of(7) {
            self.send(Event::Describe);
        }
    }

    /// The hash of everything, once the sequence is run.
    fn finish(mut self, steps: usize) -> Outcome {
        for _ in 0..steps {
            self.step();
        }
        self.group.member_count().hash(&mut self.hash);
        self.group.protocol_type().hash(&mut self.hash);
        Outcome {
            hash: self.hash.finish(),
            notes: self.notes,
            tally: self.tally,
        }
    }
}

struct Outcome {
    hash: u64,
    notes: usize,
    tally: BTreeMap<String, usize>,
}

/// The hash of the sequences of seeds `0..SEEDS`, chained, and what they did.
fn whole_run() -> (u64, usize, BTreeMap<String, usize>) {
    let mut chain = DefaultHasher::new();
    let (mut notes, mut tally) = (0, BTreeMap::new());
    for seed in 0..SEEDS {
        let outcome = Run::new(seed).finish(STEPS);
        outcome.hash.hash(&mut chain);
        notes += outcome.notes;
        for (what, n) in outcome.tally {
            *tally.entry(what).or_default() += n;
        }
    }
    (chain.finish(), notes, tally)
}

const SEEDS: u64 = 500;
const STEPS: usize = 400;

/// What the sequences must reach, or the hash says little: rebalances begun by joins, leaves and silence, a generation
/// formed and synced, a group that changes kind, a member that is fenced.
const REACHED: [&str; 9] = [
    "join:ok",
    "sync:ok",
    "beat:RebalanceInProgress",
    "describe:Stable:false",
    "describe:PreparingRebalance:false",
    "describe:Stable:true",
    "consumer:ok",
    "leave",
    "forget",
];

/// The hash of `SEEDS` sequences of `STEPS` steps, on the machine as it was before it took notes.
const BEFORE_NOTES: u64 = 4_074_371_788_659_210_500;

#[test]
fn the_machine_does_what_it_did_before_it_took_notes() {
    let (hash, _notes, tally) = whole_run();
    for what in REACHED {
        assert!(
            tally.get(what).copied().unwrap_or(0) >= 500,
            "the sequences hardly reach {what}: {tally:?}"
        );
    }
    assert_eq!(
        hash, BEFORE_NOTES,
        "{SEEDS} seeds of {STEPS} steps: the group no longer does what it did before it took notes"
    );
}
