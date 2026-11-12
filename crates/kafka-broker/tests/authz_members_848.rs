//! The same invariant as `authz_regex_848.rs` (SEC-210), checked against every OTHER source of a member's topics: a
//! static member's place taken by another principal, names that lose their DESCRIBE right, and a classic member of a
//! consumer group. No member of a group is told a topic its own principal may not DESCRIBE, however it came by it.
//! (SEC-230 to SEC-235.) The table of the places is in the catalogue under SEC-210.
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
#[path = "support/classic_848.rs"]
mod classic;
#[path = "support/regex_seat.rs"]
mod seat;

use gx::{DENY, DESCRIBE, GROUP, READ, TOPIC, allow, deny, make_topic};
use seat::{Seat, id_of, world};
use support::acls::{admin, filter, grant, user};
use support::{beat_848, join_848};

/// A rig with `secret` (3 partitions) and `open` (2); alice may DESCRIBE both, bob only `open`, both may READ the group.
async fn two_users() -> support::Rig {
    let rig = world();
    make_topic(&rig, "secret", 3).await;
    make_topic(&rig, "open", 2).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:alice", READ),
            allow(TOPIC, "secret", "User:alice", DESCRIBE),
            allow(TOPIC, "open", "User:alice", DESCRIBE),
            allow(GROUP, "g", "User:bob", READ),
            allow(TOPIC, "open", "User:bob", DESCRIBE),
        ],
    )
    .await;
    rig
}

/// bob's join to take the instance `i1` over, with an expression that matches nothing and no names.
fn takeover_by_regex(member: &str) -> kafka_wire::messages::ConsumerGroupHeartbeatRequest {
    let mut req = join_848("g", member, &[]);
    req.subscribed_topic_names = None;
    req.subscribed_topic_regex = Some(gx::text("never-matches-x"));
    req
}

/// The auditor's reproduction: a static member's names and partitions are not handed to the principal that takes its place.
#[tokio::test]
async fn static_member_replaced_by_a_principal_who_cannot_describe_inherits_the_topic() {
    let rig = two_users().await;
    let secret = id_of(&rig, "secret").await;
    let mut alice = Seat::join_names(&rig, user("alice"), "g", "ma", Some("i1"), &["secret"]).await;
    alice.beats(6).await;
    assert!(alice.owns(secret), "alice owns secret");
    alice.leave_for_now().await;
    let mut bob = Seat::start(
        &rig,
        user("bob"),
        "g",
        "mb",
        Some("i1"),
        takeover_by_regex("mb"),
    )
    .await;
    bob.beats(8).await;
    assert!(
        !bob.ever.contains(&secret),
        "bob was assigned secret: {:?}",
        bob.ever
    );
    assert!(bob.owned.is_empty(), "{:?}", bob.owned);
}

/// The same instance restarted by the same principal keeps what it had: nothing is taken from a caller that may see it.
#[tokio::test]
async fn a_static_member_restarted_by_the_same_principal_keeps_its_partitions() {
    let rig = two_users().await;
    let secret = id_of(&rig, "secret").await;
    let mut alice = Seat::join_names(&rig, user("alice"), "g", "ma", Some("i1"), &["secret"]).await;
    alice.beats(6).await;
    alice.leave_for_now().await;
    let again = Seat::start(
        &rig,
        user("alice"),
        "g",
        "ma2",
        Some("i1"),
        takeover_by_regex("ma2"),
    )
    .await;
    assert!(again.owns(secret), "{:?}", again.owned);
}

/// A principal that may DESCRIBE the topic and takes the place of a static member is given what the member had.
#[tokio::test]
async fn a_principal_that_may_describe_the_topic_inherits_it() {
    let rig = two_users().await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:carol", READ),
            allow(TOPIC, "secret", "User:carol", DESCRIBE),
        ],
    )
    .await;
    let secret = id_of(&rig, "secret").await;
    let mut alice = Seat::join_names(&rig, user("alice"), "g", "ma", Some("i1"), &["secret"]).await;
    alice.beats(6).await;
    alice.leave_for_now().await;
    let carol = Seat::start(
        &rig,
        user("carol"),
        "g",
        "mc",
        Some("i1"),
        takeover_by_regex("mc"),
    )
    .await;
    assert!(carol.owns(secret), "{:?}", carol.owned);
}

/// The names a member inherited are not shown either: ConsumerGroupDescribe of a group it is in lists only the names its
/// own principal may DESCRIBE.
#[tokio::test]
async fn the_names_a_taker_inherited_are_not_listed_for_it() {
    let rig = two_users().await;
    grant(&rig, vec![allow(GROUP, "g", "User:bob", DESCRIBE)]).await;
    let mut alice = Seat::join_names(
        &rig,
        user("alice"),
        "g",
        "ma",
        Some("i1"),
        &["secret", "open"],
    )
    .await;
    alice.beats(6).await;
    alice.leave_for_now().await;
    let mut bob = Seat::start(
        &rig,
        user("bob"),
        "g",
        "mb",
        Some("i1"),
        takeover_by_regex("mb"),
    )
    .await;
    bob.beats(6).await;
    let groups = calls::describe_848(&rig, &admin(), 1, &["g"], false).await;
    let members = &groups[0].members;
    assert_eq!(members.len(), 1, "{members:?}");
    assert!(
        members[0]
            .subscribed_topic_names
            .iter()
            .all(|n| n.to_string() != "secret"),
        "{members:?}"
    );
}

/// Names are held to the right as the expression is: a DENY issued after the subscription takes the topic away at the
/// member's next heartbeat, and lifting it brings the topic back.
#[tokio::test]
async fn names_lose_their_topic_when_describe_is_denied_later_and_regain_it() {
    let rig = two_users().await;
    let (secret, open) = (id_of(&rig, "secret").await, id_of(&rig, "open").await);
    let mut alice =
        Seat::join_names(&rig, user("alice"), "g", "ma", None, &["secret", "open"]).await;
    alice.beats(6).await;
    assert!(alice.owns(secret) && alice.owns(open), "{:?}", alice.owned);
    grant(&rig, vec![deny(TOPIC, "secret", "User:alice", DESCRIBE)]).await;
    let before = alice.ever.len();
    alice.beats(8).await;
    assert!(!alice.owns(secret), "still owns secret: {:?}", alice.owned);
    assert!(alice.owns(open), "{:?}", alice.owned);
    assert_eq!(alice.ever.len(), before, "nothing new was ever given");
    rig.delete_acls_as(&admin(), 3, vec![filter(TOPIC, 1, DESCRIBE, DENY)])
        .await;
    alice.beats(8).await;
    assert!(alice.owns(secret), "the right came back: {:?}", alice.owned);
}

/// The answer to the very heartbeat that finds a DENY already leaves the topic out.
#[tokio::test]
async fn the_heartbeat_that_finds_the_deny_does_not_carry_the_topic() {
    let rig = two_users().await;
    let secret = id_of(&rig, "secret").await;
    let mut alice = Seat::join_names(&rig, user("alice"), "g", "ma", None, &["secret"]).await;
    alice.beats(4).await;
    grant(&rig, vec![deny(TOPIC, "secret", "User:alice", DESCRIBE)]).await;
    let req = beat_848("g", "ma", alice.epoch, Some(&alice.owned));
    let r = calls::heartbeat_848(&rig, &alice.who, 1, req).await;
    // The member is told what it now owns, and that no longer holds the topic.
    let told = r.assignment.expect("told the assignment it now has");
    assert!(
        told.topic_partitions.iter().all(|t| t.topic_id != secret),
        "{told:?}"
    );
}

/// A classic member of a consumer group is given the partitions of the topics it may DESCRIBE, whatever it named.
#[tokio::test]
async fn a_classic_member_is_not_assigned_a_topic_it_cannot_describe() {
    let rig = two_users().await;
    let mut anchor = Seat::join_names(&rig, user("alice"), "g", "ma", None, &["open"]).await;
    anchor.beats(4).await;
    let bob = classic::join(&rig, &user("bob"), "g", "", None, &["secret", "open"]).await;
    assert_eq!(bob.error, 0);
    let (code, got) = bob.sync(&rig, &[]).await;
    assert_eq!(code, 0);
    assert!(got.iter().all(|(t, _)| t != "secret"), "{got:?}");
    // The control: a classic member that may DESCRIBE it is given it.
    let alice = classic::join(&rig, &user("alice"), "g", "", None, &["secret"]).await;
    let (_, got) = alice.sync(&rig, &[]).await;
    assert!(got.iter().any(|(t, _)| t == "secret"), "{got:?}");
}

/// Members of a classic group that becomes a consumer group keep only what their principals may DESCRIBE.
#[tokio::test]
async fn a_classic_group_that_is_upgraded_does_not_hand_a_hidden_topic_to_its_member() {
    let rig = two_users().await;
    let bob = classic::join(&rig, &user("bob"), "g", "", None, &["secret", "open"]).await;
    assert_eq!(bob.error, 0);
    let held = classic::assignment_bytes(&[("secret", &[0, 1, 2]), ("open", &[0, 1])]);
    let (code, got) = bob.sync(&rig, &[(&bob.id, held)]).await;
    assert_eq!((code, got.len()), (0, 2), "{got:?}");
    // alice, of the new protocol, joins: the group is a consumer group, bob in it.
    let mut alice = Seat::join_names(&rig, user("alice"), "g", "ma", None, &["open"]).await;
    alice.beats(4).await;
    let mut seen = Vec::new();
    for _ in 0..4 {
        let again =
            classic::join(&rig, &user("bob"), "g", &bob.id, None, &["secret", "open"]).await;
        let (_, got) = again.sync(&rig, &[]).await;
        seen.extend(got);
        alice.beats(2).await;
    }
    assert!(seen.iter().all(|(t, _)| t != "secret"), "{seen:?}");
    assert!(seen.iter().any(|(t, _)| t == "open"), "{seen:?}");
}

/// A classic member that took the place of a static member of the new protocol is not handed its hidden topics.
#[tokio::test]
async fn a_classic_member_replacing_a_static_member_inherits_nothing_hidden() {
    let rig = two_users().await;
    let secret = id_of(&rig, "secret").await;
    let mut alice = Seat::join_names(&rig, user("alice"), "g", "ma", Some("i1"), &["secret"]).await;
    alice.beats(6).await;
    assert!(alice.owns(secret));
    alice.leave_for_now().await;
    let bob = classic::join(&rig, &user("bob"), "g", "", Some("i1"), &["secret"]).await;
    assert_eq!(bob.error, 0);
    let (_, got) = bob.sync(&rig, &[]).await;
    assert!(got.iter().all(|(t, _)| t != "secret"), "{got:?}");
}
