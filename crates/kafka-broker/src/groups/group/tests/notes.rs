//! What a consumer group (the new protocol) says of itself: each way its epoch moves has its cause. And a group that
//! changes kind says so.

use super::sim::{Pump, settings_of};
use super::*;
use crate::groups::consumer::{TopicInfo, Topics};
use crate::groups::machine::{Answer, Event, JoinRequest, LeaveTarget, Protocol};
use crate::groups::note::{Cause, Reason, What};

const ORDERS: Uuid = Uuid::from_u128(11);

fn topics(partitions: i32) -> Topics {
    Topics::new([TopicInfo {
        name: "orders".into(),
        id: ORDERS,
        partitions,
    }])
}

fn beat(member: &str, epoch: i32, subscribed: &[&str]) -> Heartbeat {
    Heartbeat {
        group: "g".into(),
        member_id: MemberId::new(member),
        version: 1,
        epoch,
        instance: None,
        rack: None,
        rebalance_ms: 60_000,
        subscribed: (epoch == 0 || !subscribed.is_empty())
            .then(|| subscribed.iter().map(|s| (*s).to_owned()).collect()),
        regex: None,
        assignor: None,
        owned: (epoch == 0).then(Vec::new),
        client_id: format!("client-{member}"),
        client_host: "/127.0.0.1".into(),
        rights: None,
    }
}

/// The epochs that moved, as (cause, epoch, members before, members after).
fn moved(p: &Pump) -> Vec<(Cause, i32, u32, u32)> {
    p.notes
        .iter()
        .filter_map(|n| match &n.what {
            What::Epoch { why, epoch, after } => Some((why.cause, *epoch, why.before, *after)),
            _ => None,
        })
        .collect()
}

fn send(p: &mut Pump, t: &Topics, h: Heartbeat) {
    p.send(t, |token| Event::ConsumerHeartbeat(token, h));
}

#[test]
fn a_member_that_joins_is_the_cause_of_the_epoch_it_makes_and_is_named() {
    let (mut p, t) = (Pump::new(0), topics(2));
    send(&mut p, &t, beat("a", 0, &["orders"]));
    assert_eq!(moved(&p), [(Cause::MemberJoined, 2, 0, 1)]);
    let What::Epoch { why, .. } = &p.notes[0].what else {
        panic!("not an epoch")
    };
    let who = why.who.as_ref().unwrap();
    assert_eq!((who.member.as_str(), &*who.client), ("a", "client-a"));
    send(&mut p, &t, beat("b", 0, &["orders"]));
    assert_eq!(moved(&p)[1], (Cause::MemberJoined, 3, 1, 2));
}

#[test]
fn a_heartbeat_that_changes_nothing_says_nothing() {
    let (mut p, t) = (Pump::new(0), topics(2));
    send(&mut p, &t, beat("a", 0, &["orders"]));
    let before = p.notes.len();
    send(&mut p, &t, beat("a", 2, &[]));
    assert_eq!(p.notes.len(), before);
}

#[test]
fn a_member_that_leaves_is_a_leave_and_one_that_goes_silent_is_an_expiry() {
    let (mut p, t) = (Pump::new(0), topics(2));
    for m in ["a", "b", "c"] {
        send(&mut p, &t, beat(m, 0, &["orders"]));
    }
    send(&mut p, &t, beat("c", -1, &[]));
    assert_eq!(moved(&p).last(), Some(&(Cause::MemberLeft, 5, 3, 2)));
    // Nobody speaks again: the session timeout (45 s) runs out for one member and then the other.
    p.advance(&t, 50_000);
    let expired: Vec<_> = moved(&p)
        .into_iter()
        .filter(|m| m.0 == Cause::SessionExpired)
        .map(|m| (m.2, m.3))
        .collect();
    assert_eq!(expired, [(2, 1), (1, 0)]);
}

#[test]
fn a_member_that_asks_for_other_topics_is_a_subscription_that_changed() {
    let (mut p, t) = (Pump::new(0), topics(2));
    send(&mut p, &t, beat("a", 0, &["orders"]));
    send(&mut p, &t, beat("a", 2, &["orders", "payments"]));
    assert_eq!(
        moved(&p).last().map(|m| m.0),
        Some(Cause::SubscriptionChanged)
    );
}

#[test]
fn partitions_added_to_a_topic_the_members_read_is_a_topic_that_changed() {
    let mut p = Pump::new(0);
    send(&mut p, &topics(2), beat("a", 0, &["orders"]));
    send(&mut p, &topics(2), beat("b", 0, &["orders"]));
    p.send(&topics(4), |_| Event::TopicsChanged);
    let last = *moved(&p).last().unwrap();
    assert_eq!((last.0, last.2, last.3), (Cause::TopicChanged, 2, 2));
    let What::Epoch { why, .. } = &p.notes.last().unwrap().what else {
        panic!("not an epoch")
    };
    assert!(why.who.is_none(), "no member did it");
}

#[test]
fn a_topic_made_where_nobody_reads_moves_no_epoch() {
    let mut p = Pump::new(0);
    send(&mut p, &topics(2), beat("a", 0, &["orders"]));
    let before = p.notes.len();
    let elsewhere = Topics::new([
        TopicInfo {
            name: "orders".into(),
            id: ORDERS,
            partitions: 2,
        },
        TopicInfo {
            name: "elsewhere".into(),
            id: Uuid::from_u128(12),
            partitions: 3,
        },
    ]);
    p.send(&elsewhere, |_| Event::TopicsChanged);
    assert_eq!(
        p.notes.len(),
        before,
        "a topic nobody reads is not a change for this group"
    );
    p.send(&Topics::new([]), |_| Event::TopicsChanged);
    assert_eq!(
        moved(&p).last().map(|m| m.0),
        Some(Cause::TopicChanged),
        "the topic it reads was deleted"
    );
}

#[test]
fn a_member_that_does_not_give_up_what_it_was_told_to_in_time_is_a_rebalance_timeout() {
    let (mut p, t) = (Pump::new(0), topics(2));
    let mut a = Client::default();
    a.poll(&mut p, &t, "a", true);
    a.poll(&mut p, &t, "a", true);
    assert_eq!(a.owned.len(), 1, "a holds the topic: {:?}", a.owned);
    let mut b = Client::default();
    b.poll(&mut p, &t, "b", false);
    // a is told to give a partition up, and keeps saying it holds both. Its heartbeats keep its session alive until its
    // rebalance timeout (60 s) runs out.
    for _ in 0..14 {
        p.advance(&t, 5_000);
        a.poll(&mut p, &t, "a", true);
        b.poll(&mut p, &t, "b", false);
    }
    let causes: Vec<_> = moved(&p).into_iter().map(|m| m.0).collect();
    assert!(causes.contains(&Cause::RebalanceTimeout), "{causes:?}");
}

/// A member of the new protocol as a client keeps it: the epoch it was told and what it holds.
#[derive(Default)]
struct Client {
    epoch: i32,
    owned: Vec<(Uuid, Vec<i32>)>,
}

impl Client {
    /// Joins or heartbeats. A `stubborn` client never lets go of what it holds.
    fn poll(&mut self, p: &mut Pump, t: &Topics, member: &str, stubborn: bool) {
        let mut h = beat(member, self.epoch, &["orders"]);
        h.owned = Some(self.owned.clone());
        let token = p.send(t, |token| Event::ConsumerHeartbeat(token, h));
        let Some(Answer::ConsumerHeartbeat(answer)) = p.take(token) else {
            return;
        };
        if answer.error.is_some() {
            self.epoch = 0;
            return;
        }
        self.epoch = answer.epoch;
        if let Some(parts) = answer.assignment
            && !(stubborn && !self.owned.is_empty())
        {
            let mut by: std::collections::BTreeMap<Uuid, Vec<i32>> = Default::default();
            for (topic, partition) in parts {
                by.entry(topic).or_default().push(partition);
            }
            self.owned = by.into_iter().collect();
        }
    }
}

fn classic_join(member: &str, reason: Option<&str>) -> JoinRequest {
    JoinRequest {
        reason: reason.and_then(Reason::clip),
        member_id: MemberId::new(member),
        instance: None,
        protocol_type: "consumer".into(),
        protocols: vec![Protocol {
            name: "range".into(),
            metadata: Bytes::from_static(&[
                0, 0, 0, 0, 0, 1, 0, 6, b'o', b'r', b'd', b'e', b'r', b's', 255, 255, 255, 255,
            ]),
        }],
        session_ms: 10_000,
        rebalance_ms: 30_000,
        client_id: "classic".into(),
        client_host: "/127.0.0.1".into(),
        issued: true,
        id_first: false,
        rights: None,
    }
}

#[test]
fn a_classic_member_of_a_consumer_group_says_why_and_what_it_said_when_it_leaves() {
    let (mut p, t) = (Pump::new(0), topics(2));
    send(&mut p, &t, beat("a", 0, &["orders"]));
    p.send(&t, |token| {
        Event::Join(token, classic_join("k", Some("rolling upgrade")))
    });
    let joined = p.notes.iter().rev().find_map(|n| match &n.what {
        What::Epoch { why, .. } if why.cause == Cause::MemberJoined => why.who.as_ref(),
        _ => None,
    });
    let joined = joined.expect("the classic member joined");
    assert_eq!(joined.member.as_str(), "k");
    assert_eq!(
        joined.reason.as_ref().map(|r| r.as_str()),
        Some("rolling upgrade")
    );
    let leave = vec![LeaveTarget {
        reason: Reason::clip("deploy"),
        member_id: MemberId::new("k"),
        instance: None,
    }];
    p.send(&t, |token| Event::Leave(token, leave));
    let left = p.notes.iter().rev().find_map(|n| match &n.what {
        What::Epoch { why, .. } if why.cause == Cause::MemberLeft => why.who.as_ref(),
        _ => None,
    });
    assert_eq!(
        left.and_then(|w| w.reason.as_ref()).map(|r| r.as_str()),
        Some("deploy")
    );
}

#[test]
fn a_classic_group_made_a_consumer_group_by_a_member_of_the_new_protocol_says_so() {
    let (mut p, t) = (Pump::new(0), topics(2));
    p.group = Group::classic(settings_of(0));
    p.send(&t, |token| Event::Join(token, classic_join("old", None)));
    send(&mut p, &t, beat("new", 0, &["orders"]));
    let causes: Vec<_> = moved(&p).into_iter().map(|m| m.0).collect();
    assert_eq!(causes, [Cause::BecameConsumer, Cause::MemberJoined]);
}
