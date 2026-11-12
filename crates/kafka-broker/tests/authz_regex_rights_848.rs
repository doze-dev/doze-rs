//! A regex subscription follows the member's rights as they change (SEC-211): a right granted or taken away is seen at
//! the next heartbeat, with no expression sent again; a topic made later is judged by the same rule; an ACL change that
//! changes nothing the pattern takes does not move the group's epoch.
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
use seat::{Seat, id_of, world};
use support::acls::{admin, grant, user};

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

/// The expression matches only a topic the member may not describe: it holds nothing, and the right, once granted, is
/// the only thing that has changed what the pattern takes (so the epoch must move for it alone).
#[tokio::test]
async fn a_right_granted_on_the_only_topic_the_expression_matches_is_taken_up() {
    let rig = world();
    make_topic(&rig, "secret", 3).await;
    grant(&rig, vec![allow(GROUP, "g", "User:alice", READ)]).await;
    let secret = id_of(&rig, "secret").await;
    let mut seat = Seat::join_regex(&rig, user("alice"), "g", "m", "sec.*").await;
    assert!(seat.owned.is_empty() && seat.ever.is_empty());
    grant(&rig, vec![allow(TOPIC, "secret", "User:alice", DESCRIBE)]).await;
    seat.beats(6).await;
    assert!(seat.owns(secret), "{:?}", seat.owned);
}

/// An ACL change that leaves what the pattern takes as it was does not move the group's epoch (the new snapshot is a
/// new value, but the topics it takes are the same).
#[tokio::test]
async fn an_acl_change_that_leaves_the_matches_alone_does_not_move_the_group_epoch() {
    let rig = world();
    make_topic(&rig, "open", 2).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:alice", READ),
            allow(TOPIC, "open", "User:alice", DESCRIBE),
        ],
    )
    .await;
    let mut seat = Seat::join_regex(&rig, user("alice"), "g", "m", ".*").await;
    let epoch =
        || async { calls::describe_848(&rig, &admin(), 1, &["g"], false).await[0].group_epoch };
    let before = epoch().await;
    grant(&rig, vec![allow(GROUP, "other", "User:bob", READ)]).await;
    seat.beats(4).await;
    assert_eq!(epoch().await, before);
}
