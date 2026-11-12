//! A KIP-848 regex subscription is resolved with the rights of the member that makes it: Kafka's
//! `TopicRegexResolver.filterTopicDescribeAuthorizedTopics` (`group-coordinator/.../modern/consumer/TopicRegexResolver.java`
//! 125-166, called from `GroupMetadataManager.java` 3379) removes every topic the requester may not DESCRIBE from what
//! the expression matches, so that the existence, the id and the partition count of a hidden topic are never told.
//! (SEC-210, SEC-212, SEC-213.) The rights that change are in `authz_regex_rights_848.rs`.
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
#[path = "support/regex_seat.rs"]
mod seat;

use gx::{DENY, DESCRIBE, GROUP, READ, TOPIC, allow, creation, make_topic};
use kafka_wire::Uuid;
use seat::{Seat, id_of, world};
use std::time::Duration;
use support::acls::{admin, grant, user};
use support::rig_with;

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

/// The group's subscribed set (what OffsetDelete refuses with GROUP_SUBSCRIBED_TO_TOPIC) holds the topics the pattern
/// takes, not every topic it matches: a topic the member may not describe is not one the group is subscribed to.
#[tokio::test]
async fn the_groups_subscribed_set_holds_only_what_the_pattern_takes() {
    let rig = world();
    make_topic(&rig, "secret", 1).await;
    make_topic(&rig, "open", 1).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:alice", READ),
            allow(TOPIC, "open", "User:alice", DESCRIBE),
        ],
    )
    .await;
    let _seat = Seat::join_regex(&rig, user("alice"), "g", "m", ".*").await;
    let r = calls::offset_delete(&rig, &admin(), "g", &[("open", 0), ("secret", 0)]).await;
    let codes: Vec<_> = r
        .topics
        .iter()
        .map(|t| (t.name.as_str().to_owned(), t.partitions[0].error_code))
        .collect();
    assert_eq!(codes.len(), 2, "{codes:?}");
    for (topic, code) in &codes {
        // 86: the group is subscribed to it.
        assert_eq!(*code == 86, topic == "open", "{codes:?}");
    }
}
