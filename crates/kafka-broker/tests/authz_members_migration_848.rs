//! A classic group becoming a consumer group and a consumer group becoming a classic one again carry each member's
//! rights with it, and the group's subscribed set counts only what its members may have (SEC-232, SEC-234).
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

use gx::{DESCRIBE, GROUP, READ, TOPIC, allow, deny, make_topic};
use seat::{Seat, world};
use support::acls::{admin, grant, user};
use support::beat_848;

async fn rig() -> support::Rig {
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
            allow(GROUP, "g", "User:bob", DESCRIBE),
            allow(TOPIC, "open", "User:bob", DESCRIBE),
        ],
    )
    .await;
    rig
}

/// bob, who may not DESCRIBE `secret`, is a classic member of `g` that holds `secret` and `open`.
async fn bob_holds_both(rig: &support::Rig) -> classic::Classic {
    let bob = classic::join(rig, &user("bob"), "g", "", None, &["secret", "open"]).await;
    let held = classic::assignment_bytes(&[("secret", &[0, 1, 2]), ("open", &[0, 1])]);
    let (code, got) = bob.sync(rig, &[(&bob.id, held)]).await;
    assert_eq!((code, got.len()), (0, 2), "{got:?}");
    bob
}

/// What bob is shown of the group: his member's names, assignment and target, by topic name. The describe is refused as a
/// whole (29) if a member holds a topic the describer may not DESCRIBE, so a clean answer is itself the proof.
async fn shown_to_bob(rig: &support::Rig, member: &str) -> Vec<String> {
    let groups = calls::describe_848(rig, &user("bob"), 1, &["g"], false).await;
    assert_eq!(groups[0].error_code, 0, "{:?}", groups[0].error_message);
    let m = groups[0]
        .members
        .iter()
        .find(|m| m.member_id.to_string() == member)
        .unwrap_or_else(|| panic!("{member} is not shown: {:?}", groups[0].members));
    let mut seen: Vec<String> = m
        .subscribed_topic_names
        .iter()
        .map(|n| n.to_string())
        .collect();
    for a in [&m.assignment, &m.target_assignment] {
        seen.extend(a.topic_partitions.iter().map(|t| t.topic_name.to_string()));
    }
    seen
}

#[tokio::test]
async fn an_upgraded_group_shows_a_classic_member_only_what_its_principal_may_describe() {
    let rig = rig().await;
    let bob = bob_holds_both(&rig).await;
    let mut alice = Seat::join_names(&rig, user("alice"), "g", "ma", None, &["open"]).await;
    alice.beats(2).await;
    // Bob has not spoken since the upgrade: what the group made of him is already held to his rights.
    let seen = shown_to_bob(&rig, &bob.id).await;
    assert!(seen.iter().all(|t| t != "secret"), "{seen:?}");
    assert!(seen.iter().any(|t| t == "open"), "{seen:?}");
}

#[tokio::test]
async fn the_rights_survive_the_group_becoming_classic_and_a_consumer_group_again() {
    let rig = rig().await;
    let bob = bob_holds_both(&rig).await;
    let mut alice = Seat::join_names(&rig, user("alice"), "g", "ma", None, &["open"]).await;
    alice.beats(2).await;
    // The last member of the new protocol goes: the group is classic again, bob in it.
    let leave = beat_848("g", "ma", -1, None);
    assert_eq!(alice.send(leave).await, 0);
    let described = calls::describe_848(&rig, &admin(), 1, &["g"], false).await;
    assert_ne!(described[0].error_code, 0, "no longer a consumer group");
    // A member of the new protocol joins again: the group is upgraded a second time.
    let _again = Seat::join_names(&rig, user("alice"), "g", "mb", None, &["open"]).await;
    let seen = shown_to_bob(&rig, &bob.id).await;
    assert!(seen.iter().all(|t| t != "secret"), "{seen:?}");
}

/// The group's subscribed set (what OffsetDelete refuses with GROUP_SUBSCRIBED_TO_TOPIC) holds the names a member may
/// have, not every name it sent: a topic it may no longer describe is not one the group is subscribed to.
#[tokio::test]
async fn the_groups_subscribed_set_drops_a_name_whose_right_was_taken_away() {
    let rig = rig().await;
    let mut alice =
        Seat::join_names(&rig, user("alice"), "g", "ma", None, &["secret", "open"]).await;
    alice.beats(2).await;
    grant(&rig, vec![deny(TOPIC, "secret", "User:alice", DESCRIBE)]).await;
    alice.beats(4).await;
    let r = calls::offset_delete(&rig, &admin(), "g", &[("open", 0), ("secret", 0)]).await;
    let codes: Vec<_> = r
        .topics
        .iter()
        .map(|t| (t.name.as_str().to_owned(), t.partitions[0].error_code))
        .collect();
    assert_eq!(codes.len(), 2, "{codes:?}");
    for (topic, code) in &codes {
        assert_eq!(*code == 86, topic == "open", "{codes:?}");
    }
}
