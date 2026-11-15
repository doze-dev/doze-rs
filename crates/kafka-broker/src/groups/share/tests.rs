//! The share group's rules, driven by a number for a clock: Kafka 4.4.0's `shareGroupHeartbeat` (docs/port/behaviours/
//! share-groups.md, SG-20 to SG-29).

use std::collections::BTreeMap;

use kafka_wire::Uuid;
use kafka_wire::ids::MemberId;

use super::*;
use crate::groups::consumer::TopicInfo;

const T: Uuid = Uuid::from_u128(1);
const U: Uuid = Uuid::from_u128(2);
const TIMING: ShareTiming = ShareTiming {
    session_ms: 45_000,
    heartbeat_ms: 5_000,
    assign_interval_ms: 1_000,
    max_size: 3,
};

fn topics(t: i32) -> Topics {
    Topics::new([
        TopicInfo {
            name: "t".into(),
            id: T,
            partitions: t,
        },
        TopicInfo {
            name: "u".into(),
            id: U,
            partitions: 1,
        },
    ])
}

fn id(s: &str) -> MemberId {
    MemberId::new(s).unwrap()
}

fn hb(member: &str, epoch: i32, subscribed: Option<&[&str]>) -> ShareHeartbeat {
    ShareHeartbeat {
        group: "g".into(),
        member_id: id(member),
        epoch,
        rack: None,
        subscribed: subscribed.map(|s| s.iter().map(|n| (*n).to_owned()).collect()),
        client_id: "c".into(),
        client_host: "/127.0.0.1".into(),
        timing: TIMING,
    }
}

struct Rig {
    g: ShareGroup,
    topics: Topics,
    now: u64,
    timers: BTreeMap<String, (u64, TimerKey)>,
}

fn rig() -> Rig {
    Rig {
        g: ShareGroup::new(),
        topics: topics(3),
        now: 0,
        timers: BTreeMap::new(),
    }
}

impl Rig {
    fn send(&mut self, h: &ShareHeartbeat) -> ShareAnswer {
        let mut out = Vec::new();
        self.g
            .heartbeat(Token(1), h, &self.topics, Millis(self.now), &mut out);
        self.apply(out).expect("an answer")
    }

    fn apply(&mut self, out: Vec<Effect>) -> Option<ShareAnswer> {
        let mut answer = None;
        for e in out {
            match e {
                Effect::Reply(_, Answer::ShareHeartbeat(a)) => answer = Some(a),
                Effect::Arm { key, at } => {
                    if let TimerKind::Session(m) = &key.kind {
                        self.timers.insert(m.as_str().to_owned(), (at.0, key));
                    }
                }
                Effect::Disarm(TimerKind::Session(m)) => {
                    self.timers.remove(m.as_str());
                }
                _ => {}
            }
        }
        answer
    }

    /// Moves the clock on, running out the timers that are due.
    fn wait(&mut self, ms: u64) {
        self.now += ms;
        let due: Vec<TimerKey> = self
            .timers
            .values()
            .filter(|(at, _)| *at <= self.now)
            .map(|(_, k)| k.clone())
            .collect();
        for key in due {
            let mut out = Vec::new();
            self.g.timer(&key, Millis(self.now), &mut out);
            self.apply(out);
        }
    }
}

fn parts(a: &ShareAnswer) -> Option<Vec<(Uuid, i32)>> {
    a.assignment.as_ref().map(|p| p.iter().copied().collect())
}

#[test]
fn a_member_joins_at_the_first_epoch_with_every_partition_and_is_told_only_changes() {
    let mut r = rig();
    let a = r.send(&hb("m1", 0, Some(&["t"])));
    assert_eq!((a.error.clone(), a.epoch, a.heartbeat_ms), (None, 1, 5_000));
    assert_eq!(parts(&a), Some(vec![(T, 0), (T, 1), (T, 2)]));
    assert_eq!(
        a.initialize,
        Some((1, ["t".to_owned()].into())),
        "its topics are to be initialized"
    );
    let again = r.send(&hb("m1", 1, None));
    assert_eq!(
        (again.epoch, parts(&again), again.initialize),
        (1, None, None)
    );
    // Saying what it subscribes to, even unchanged, is answered with the assignment.
    let told = r.send(&hb("m1", 1, Some(&["t"])));
    assert_eq!(parts(&told), Some(vec![(T, 0), (T, 1), (T, 2)]));
    assert_eq!(r.g.epochs(), (1, 1), "nothing changed: no new epoch");
}

#[test]
fn a_second_member_shares_after_the_assignment_interval_and_partitions_are_spread() {
    let mut r = rig();
    r.send(&hb("a", 0, Some(&["t"])));
    let b = r.send(&hb("b", 0, Some(&["t"])));
    assert_eq!(
        (b.epoch, parts(&b)),
        (1, Some(vec![])),
        "within the interval: the targets of epoch 1 stand"
    );
    assert_eq!(r.g.epochs(), (2, 1));
    r.now += 1_000;
    let b = r.send(&hb("b", 1, None));
    assert_eq!(b.epoch, 2);
    let a = r.send(&hb("a", 1, None));
    assert_eq!(a.epoch, 2);
    let held: usize = [parts(&a), parts(&b)].iter().flatten().map(Vec::len).sum();
    assert_eq!(held, 3, "every partition held once: {a:?} {b:?}");
    assert_eq!(parts(&a).unwrap().len(), 2, "a keeps two of its three");
}

#[test]
fn the_checks_of_a_known_members_epoch_are_kafkas() {
    let mut r = rig();
    r.send(&hb("a", 0, Some(&["t"])));
    let greater = r.send(&hb("a", 7, None));
    let (code, words) = greater.error.unwrap();
    assert_eq!(code, E::FencedMemberEpoch);
    assert!(
        words.starts_with("The share group member has a greater member epoch (7) than"),
        "{words}"
    );
    // A subscription change moves the epoch; the epoch before stays good once (an answer lost).
    r.now += 1_000;
    let moved = r.send(&hb("a", 1, Some(&["t", "u"])));
    assert_eq!(moved.epoch, 2);
    assert!(
        r.send(&hb("a", 1, None)).error.is_none(),
        "the previous epoch"
    );
    let rejoined = r.send(&hb("a", 0, None));
    assert!(rejoined.error.is_none(), "0 rejoins, keeping the epoch");
    assert_eq!(
        (rejoined.epoch, parts(&rejoined).map(|p| p.len())),
        (2, Some(4))
    );
    let unknown = r.send(&hb("x", 3, None));
    assert_eq!(
        unknown.error,
        Some((
            E::UnknownMemberId,
            "Member x is not a member of group g.".into()
        ))
    );
}

#[test]
fn a_smaller_epoch_that_is_not_the_previous_one_is_fenced() {
    let mut r = rig();
    r.send(&hb("a", 0, Some(&["t"])));
    for subs in [["u"], ["t"]] {
        r.now += 1_000;
        let e = r.g.epochs().0;
        r.send(&hb("a", e, Some(&subs)));
    }
    let (prev, cur) = (2, 3);
    assert_eq!(r.g.epochs().0, cur);
    let (code, words) = r.send(&hb("a", 1, None)).error.unwrap();
    assert_eq!(code, E::FencedMemberEpoch);
    assert!(
        words.contains(&format!("smaller member epoch (1) than the one known by the group coordinator ({cur}), and it does not match the previous member epoch ({prev})")),
        "{words}"
    );
}

#[test]
fn the_group_is_full_at_its_maximum_size_but_a_member_already_in_it_is_not_refused() {
    let mut r = rig();
    for m in ["a", "b", "c"] {
        assert!(r.send(&hb(m, 0, Some(&["t"]))).error.is_none());
    }
    let (code, words) = r.send(&hb("d", 0, Some(&["t"]))).error.unwrap();
    assert_eq!(code, E::GroupMaxSizeReached);
    assert_eq!(
        words,
        "The share group has reached its maximum capacity of 3 members."
    );
    assert!(
        r.send(&hb("a", 0, Some(&["t"]))).error.is_none(),
        "a rejoin"
    );
}

#[test]
fn leaving_and_a_silent_member_move_the_epoch_and_the_last_one_leaves_the_targets_at_it() {
    let mut r = rig();
    r.send(&hb("a", 0, Some(&["t"])));
    r.send(&hb("b", 0, Some(&["t"])));
    let left = r.send(&hb("a", -1, None));
    assert_eq!(
        (left.epoch, left.heartbeat_ms, left.assignment),
        (-1, 0, None)
    );
    assert_eq!(r.g.epochs(), (3, 1));
    let again = r.send(&hb("a", -1, None));
    assert_eq!(
        again.error.unwrap().0,
        E::UnknownMemberId,
        "no epoch check, but it must be there"
    );
    r.wait(45_000);
    assert_eq!(r.g.member_count(), 0, "b's session ran out");
    assert_eq!(
        r.g.epochs(),
        (4, 4),
        "the last member gone: the (empty) targets are the new epoch's"
    );
}

#[test]
fn a_topic_that_grows_moves_the_epoch_at_the_next_heartbeat_and_the_new_partition_is_given_out() {
    let mut r = rig();
    r.send(&hb("a", 0, Some(&["t"])));
    r.topics = topics(4);
    r.now += 1_000;
    let a = r.send(&hb("a", 1, None));
    assert_eq!(a.epoch, 2);
    assert_eq!(parts(&a).map(|p| p.len()), Some(4));
    assert_eq!(
        a.initialize.map(|i| i.0),
        Some(2),
        "the new partition is to be initialized"
    );
}

#[test]
fn a_subscription_that_drops_a_topic_drops_its_partitions_at_once() {
    let mut r = rig();
    r.send(&hb("a", 0, Some(&["t", "u"])));
    r.now += 1;
    let a = r.send(&hb("a", 1, Some(&["u"])));
    assert_eq!(
        parts(&a),
        Some(vec![(U, 0)]),
        "filtered before the next targets are computed"
    );
}

#[test]
fn a_member_reading_a_topic_that_is_not_there_is_given_nothing_of_it() {
    let mut r = rig();
    let a = r.send(&hb("a", 0, Some(&["nope", "u"])));
    assert_eq!(parts(&a), Some(vec![(U, 0)]));
    assert_eq!(r.g.view(&r.topics).members[0].subscribed, ["nope", "u"]);
}
