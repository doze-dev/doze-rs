//! A KIP-848 regex subscription is resolved with the rights of the member that makes it: Kafka's
//! `TopicRegexResolver.filterTopicDescribeAuthorizedTopics` (`group-coordinator/.../modern/consumer/TopicRegexResolver.java`
//! 125-166, called from `GroupMetadataManager.java` 3379) removes every topic the requester may not DESCRIBE from what
//! the expression matches, so that the existence, the id and the partition count of a hidden topic are never told.
//! (SEC-210 to SEC-213.)
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
#[macro_use]
#[path = "support/authz_groups.rs"]
mod gx;
#[path = "support/authz_calls.rs"]
mod calls;

use std::collections::BTreeSet;
use std::time::Duration;

use gx::{DENY, DESCRIBE, GROUP, READ, TOPIC, allow, creation, make_topic};
use kafka_broker::Caller;
use kafka_wire::Uuid;
use support::acls::{admin, grant, user};
use support::{Rig, beat_848, join_848, rig_with};

/// A broker that refuses by default, whose assignments are computed at every heartbeat.
fn world() -> Rig {
    rig_with(|c| {
        gx::closed(c);
        c.consumer_assignment_interval = Duration::ZERO;
    })
}

async fn id_of(rig: &Rig, name: &str) -> Uuid {
    rig.metadata_as(&admin(), 12, Some(&[name]), false)
        .await
        .topics[0]
        .topic_id
}

/// A member of the new protocol that keeps on heartbeating, owning what it was told to.
struct Seat<'a> {
    rig: &'a Rig,
    who: Caller,
    group: &'static str,
    member: &'static str,
    epoch: i32,
    owned: Vec<(Uuid, Vec<i32>)>,
    /// Every topic id it was ever assigned a partition of.
    ever: BTreeSet<Uuid>,
}

impl<'a> Seat<'a> {
    /// Joins with the regex (and no names).
    async fn join_regex(
        rig: &'a Rig,
        who: Caller,
        group: &'static str,
        member: &'static str,
        regex: &str,
    ) -> Seat<'a> {
        let mut req = join_848(group, member, &[]);
        req.subscribed_topic_names = None;
        req.subscribed_topic_regex = Some(gx::text(regex));
        let mut seat = Seat {
            rig,
            who,
            group,
            member,
            epoch: 0,
            owned: Vec::new(),
            ever: BTreeSet::new(),
        };
        let r = seat.send(req).await;
        assert_eq!(r, 0, "the join");
        seat.beats(4).await;
        seat
    }

    async fn send(&mut self, req: kafka_wire::messages::ConsumerGroupHeartbeatRequest) -> i16 {
        let r = calls::heartbeat_848(self.rig, &self.who, 1, req).await;
        if r.error_code == 0 {
            self.epoch = r.member_epoch;
            if let Some(a) = &r.assignment {
                self.owned = a
                    .topic_partitions
                    .iter()
                    .map(|t| (t.topic_id, t.partitions.clone()))
                    .collect();
                self.ever.extend(self.owned.iter().map(|(id, _)| *id));
            }
        }
        r.error_code
    }

    /// `n` plain heartbeats, each saying what the member owns.
    async fn beats(&mut self, n: usize) {
        for _ in 0..n {
            let owned = self.owned.clone();
            let req = beat_848(self.group, self.member, self.epoch, Some(&owned));
            assert_eq!(self.send(req).await, 0);
        }
    }

    fn owns(&self, id: Uuid) -> bool {
        self.owned.iter().any(|(t, _)| *t == id)
    }
}

/// The auditor's reproduction: READ on the group and DESCRIBE only on `open`; the regex `.*` must never bring `secret`.
#[tokio::test]
async fn a_regex_is_assigned_only_topics_the_member_may_describe() {
    let rig = world();
    make_topic(&rig, "secret", 3).await;
    make_topic(&rig, "open", 2).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:alice", READ),
            allow(TOPIC, "open", "User:alice", DESCRIBE),
        ],
    )
    .await;
    let (secret, open) = (id_of(&rig, "secret").await, id_of(&rig, "open").await);
    let seat = Seat::join_regex(&rig, user("alice"), "g", "m", ".*").await;
    assert!(
        !seat.ever.contains(&secret),
        "assigned `secret`: {:?}",
        seat.ever
    );
    assert!(seat.owns(open), "the topic she may describe is hers");
    // The partitions are the whole of `open`.
    assert_eq!(seat.owned, vec![(open, vec![0, 1])]);
}

/// The expression matches describable topics only: nothing is taken away from what it would have matched anyway.
#[tokio::test]
async fn a_regex_that_matches_only_describable_topics_gets_all_of_them() {
    let rig = world();
    make_topic(&rig, "orders-a", 2).await;
    make_topic(&rig, "orders-b", 1).await;
    make_topic(&rig, "secret", 3).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:alice", READ),
            creation(
                (TOPIC, "orders-", gx::PREFIXED),
                "User:alice",
                DESCRIBE,
                gx::ALLOW,
            ),
        ],
    )
    .await;
    let seat = Seat::join_regex(&rig, user("alice"), "g", "m", "orders-.*").await;
    let (a, b) = (id_of(&rig, "orders-a").await, id_of(&rig, "orders-b").await);
    let secret = id_of(&rig, "secret").await;
    assert_eq!(seat.owned.len(), 2, "{:?}", seat.owned);
    assert!(seat.owned.contains(&(a, vec![0, 1])) && seat.owned.contains(&(b, vec![0])));
    assert!(!seat.ever.contains(&secret));
}

/// A right granted later is picked up by the next heartbeat, which does not carry the regex again.
#[tokio::test]
async fn a_describe_right_granted_later_is_taken_up_by_the_next_heartbeat() {
    let rig = world();
    make_topic(&rig, "secret", 3).await;
    make_topic(&rig, "open", 2).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:alice", READ),
            allow(TOPIC, "open", "User:alice", DESCRIBE),
        ],
    )
    .await;
    let secret = id_of(&rig, "secret").await;
    let mut seat = Seat::join_regex(&rig, user("alice"), "g", "m", ".*").await;
    assert!(!seat.owns(secret));
    grant(&rig, vec![allow(TOPIC, "secret", "User:alice", DESCRIBE)]).await;
    seat.beats(6).await;
    assert!(seat.owns(secret), "the right was granted: {:?}", seat.owned);
}

/// A right taken away is taken away from the assignment too, at the next heartbeat.
#[tokio::test]
async fn a_describe_right_taken_away_revokes_the_topic() {
    let rig = world();
    make_topic(&rig, "secret", 3).await;
    make_topic(&rig, "open", 2).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:alice", READ),
            allow(TOPIC, "*", "User:alice", DESCRIBE),
        ],
    )
    .await;
    let (secret, open) = (id_of(&rig, "secret").await, id_of(&rig, "open").await);
    let mut seat = Seat::join_regex(&rig, user("alice"), "g", "m", ".*").await;
    assert!(seat.owns(secret) && seat.owns(open));
    grant(
        &rig,
        vec![creation(
            (TOPIC, "secret", gx::LITERAL),
            "User:alice",
            DESCRIBE,
            DENY,
        )],
    )
    .await;
    seat.beats(6).await;
    assert!(!seat.owns(secret), "{:?}", seat.owned);
    assert!(seat.owns(open));
}

/// A DENY of one topic among an ALLOW of all of them.
#[tokio::test]
async fn a_deny_keeps_one_topic_out_of_the_match() {
    let rig = world();
    make_topic(&rig, "secret", 3).await;
    make_topic(&rig, "open", 2).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:alice", READ),
            allow(TOPIC, "*", "User:alice", DESCRIBE),
            creation((TOPIC, "sec", gx::PREFIXED), "User:alice", DESCRIBE, DENY),
        ],
    )
    .await;
    let (secret, open) = (id_of(&rig, "secret").await, id_of(&rig, "open").await);
    let seat = Seat::join_regex(&rig, user("alice"), "g", "m", ".*").await;
    assert!(seat.owns(open));
    assert!(!seat.ever.contains(&secret), "{:?}", seat.ever);
}

/// A super user sees everything, a DENY notwithstanding.
#[tokio::test]
async fn a_super_user_is_assigned_every_topic_the_expression_matches() {
    let rig = world();
    make_topic(&rig, "secret", 3).await;
    make_topic(&rig, "open", 2).await;
    grant(
        &rig,
        vec![creation(
            (TOPIC, "secret", gx::LITERAL),
            "User:admin",
            DESCRIBE,
            DENY,
        )],
    )
    .await;
    let (secret, open) = (id_of(&rig, "secret").await, id_of(&rig, "open").await);
    let seat = Seat::join_regex(&rig, admin(), "g", "m", ".*").await;
    assert!(seat.owns(secret) && seat.owns(open), "{:?}", seat.owned);
}

/// Two members of one group, one expression: each is given what it may describe, and the one that may not does not
/// get a partition of the other's topic.
#[tokio::test]
async fn two_members_with_one_expression_are_filtered_each_by_its_own_rights() {
    let rig = world();
    make_topic(&rig, "secret", 4).await;
    make_topic(&rig, "open", 4).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:alice", READ),
            allow(TOPIC, "open", "User:alice", DESCRIBE),
        ],
    )
    .await;
    let (secret, open) = (id_of(&rig, "secret").await, id_of(&rig, "open").await);
    // The super user arrives first, so that its resolution is the one in force when the other joins.
    let mut boss = Seat::join_regex(&rig, admin(), "g", "boss", ".*").await;
    let mut alice = Seat::join_regex(&rig, user("alice"), "g", "alice", ".*").await;
    for _ in 0..8 {
        boss.beats(1).await;
        alice.beats(1).await;
    }
    assert!(!alice.ever.contains(&secret), "{:?}", alice.ever);
    assert!(alice.owns(open));
    assert!(boss.owns(secret), "{:?}", boss.owned);
    // What ConsumerGroupDescribe shows to the one who may describe both, for each member.
    let d = &calls::describe_848(&rig, &admin(), 1, &["g"], false).await[0];
    let alices = d
        .members
        .iter()
        .find(|m| m.member_id.as_str() == "alice")
        .unwrap();
    let hers: Vec<Uuid> = alices
        .assignment
        .topic_partitions
        .iter()
        .map(|t| t.topic_id)
        .collect();
    assert!(!hers.contains(&secret), "{hers:?}");
}

/// A member of the classic protocol is not touched: it names its topics, as it always did, and is served.
#[tokio::test]
async fn a_classic_member_in_a_group_with_a_regex_member_is_unaffected() {
    let rig = world();
    make_topic(&rig, "secret", 3).await;
    make_topic(&rig, "open", 2).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:alice", READ),
            allow(GROUP, "g", "User:carl", READ),
            allow(TOPIC, "open", "User:alice", DESCRIBE),
        ],
    )
    .await;
    let secret = id_of(&rig, "secret").await;
    let mut alice = Seat::join_regex(&rig, user("alice"), "g", "alice", ".*").await;
    // Carl joins by the classic protocol, subscribing to both topics.
    let meta = support::subscription(&["secret", "open"]);
    let carl = user("carl");
    let ask = |member: &str| support::join_request("g", member, None, &[("range", &meta)]);
    let first = match rig
        .ask_as(&carl, 4, kafka_wire::api::Request::JoinGroup(ask("")))
        .await
    {
        kafka_wire::api::Response::JoinGroup(r) => r,
        other => panic!("{other:?}"),
    };
    assert_eq!(first.error_code, 79);
    let id = first.member_id.to_string();
    let joined = match rig
        .ask_as(&carl, 4, kafka_wire::api::Request::JoinGroup(ask(&id)))
        .await
    {
        kafka_wire::api::Response::JoinGroup(r) => r,
        other => panic!("{other:?}"),
    };
    assert_eq!(joined.error_code, 0, "{joined:?}");
    alice.beats(8).await;
    assert!(!alice.ever.contains(&secret), "{:?}", alice.ever);
}

/// A topic made after the member joined is taken or left by the same rule.
#[tokio::test]
async fn a_topic_made_later_is_taken_only_if_the_member_may_describe_it() {
    let rig = world();
    make_topic(&rig, "orders-a", 1).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:alice", READ),
            creation(
                (TOPIC, "orders-", gx::PREFIXED),
                "User:alice",
                DESCRIBE,
                gx::ALLOW,
            ),
        ],
    )
    .await;
    let mut seat = Seat::join_regex(&rig, user("alice"), "g", "m", "orders-.*").await;
    make_topic(&rig, "orders-b", 2).await;
    make_topic(&rig, "orders-hidden", 2).await;
    grant(
        &rig,
        vec![creation(
            (TOPIC, "orders-hidden", gx::LITERAL),
            "User:alice",
            DESCRIBE,
            DENY,
        )],
    )
    .await;
    seat.beats(8).await;
    let (a, b) = (id_of(&rig, "orders-a").await, id_of(&rig, "orders-b").await);
    let hidden = id_of(&rig, "orders-hidden").await;
    assert!(seat.owns(a) && seat.owns(b), "{:?}", seat.owned);
    assert!(!seat.ever.contains(&hidden));
}

/// The broker whose default allows and holds no ACL narrows nobody; one ACL denying a topic narrows its caller only.
#[tokio::test]
async fn on_an_open_broker_one_deny_hides_one_topic_from_one_caller() {
    let rig = rig_with(|c| {
        gx::open(c);
        c.consumer_assignment_interval = Duration::ZERO;
    });
    make_topic(&rig, "secret", 3).await;
    make_topic(&rig, "open", 2).await;
    let (secret, open) = (id_of(&rig, "secret").await, id_of(&rig, "open").await);
    let everything = Seat::join_regex(&rig, user("bob"), "g1", "m", ".*").await;
    assert!(everything.owns(secret) && everything.owns(open));
    grant(
        &rig,
        vec![creation(
            (TOPIC, "secret", gx::LITERAL),
            "User:alice",
            DESCRIBE,
            DENY,
        )],
    )
    .await;
    let alice = Seat::join_regex(&rig, user("alice"), "g2", "m", ".*").await;
    assert!(
        alice.owns(open) && !alice.ever.contains(&secret),
        "{:?}",
        alice.ever
    );
    // Bob is not named by the ACL, but once a topic has one the default no longer allows it to anybody (Kafka's rule):
    // he keeps `open` and loses `secret` at his next heartbeat.
    let mut bob = everything;
    bob.beats(6).await;
    assert!(bob.owns(open) && !bob.owns(secret), "{:?}", bob.owned);
}
