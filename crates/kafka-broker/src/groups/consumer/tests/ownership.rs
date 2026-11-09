//! A partition is never in two members' hands: it moves only when its holder has said it has let go.

use super::*;

/// Two members, the first holding all four partitions, the second just joined past the interval: its target is two.
fn two_members() -> Rig {
    let mut r = rig(4);
    r.beat(join("a"));
    r.advance(1_500);
    let b = r.beat(join("b"));
    assert_eq!(
        b.assignment,
        Some(parts(&[])),
        "a holds everything until it gives some up"
    );
    r
}

#[test]
fn a_member_is_told_to_give_up_before_another_is_given_what_it_holds() {
    let mut r = two_members();
    // a's next heartbeat: it is to keep two, and revoke two. Its epoch does not move.
    let a = r.beat(poll_owning("a", 2, &[0, 1, 2, 3]));
    assert_eq!((a.epoch, a.assignment), (2, Some(parts(&[0, 1]))));
    assert_eq!(
        r.member("a").assigned,
        parts(&[0, 1, 2, 3]),
        "still its own until it says"
    );
    // b heartbeats: nothing is free yet.
    let b = r.beat(poll("b", 3));
    assert_eq!(b.assignment, None);
    assert_eq!(r.member("b").assigned, parts(&[]));
    // a says it has let go: they are b's to take on b's next heartbeat.
    let a = r.beat(poll_owning("a", 2, &[0, 1]));
    assert_eq!(a.epoch, 3, "a is now at the epoch of the targets");
    let b = r.beat(poll("b", 3));
    assert_eq!(b.assignment, Some(parts(&[2, 3])));
    r.advance(0);
    assert_eq!(r.view().state, "Stable");
}

#[test]
fn a_member_that_does_not_say_it_gave_up_is_removed_at_its_rebalance_timeout_and_the_partitions_freed()
 {
    let mut r = two_members();
    r.beat(poll_owning("a", 2, &[0, 1, 2, 3]));
    // a heartbeats but never reports what it owns; b keeps heartbeating, so only a's revocation can end it.
    let mut waited = 0;
    while r.view().members.iter().any(|m| m.id.as_str() == "a") {
        assert!(
            waited <= 60_000,
            "a is still a member after its rebalance timeout (60 s)"
        );
        r.advance(5_000);
        waited += 5_000;
        r.beat(poll("b", 3));
        r.beat(poll("a", 2));
    }
    assert!(
        waited >= 55_000,
        "removed after {waited} ms, not before its rebalance timeout"
    );
    // The removal moves the epoch; the next heartbeat past the interval computes the targets, and b gets all four.
    let b = r.beat(poll("b", 3));
    assert!(b.error.is_none());
    r.advance(1_500);
    r.beat(poll("b", 3));
    let epoch = r.member("b").epoch;
    r.beat(poll("b", epoch));
    assert_eq!(r.member("b").assigned, parts(&[0, 1, 2, 3]));
}

#[test]
fn a_rejoin_at_epoch_0_lets_go_of_what_the_member_held_so_that_another_may_take_it_at_once() {
    let mut r = two_members();
    // a still holds all four but is to keep two. Joining again, as an eager consumer does, it holds nothing and is
    // given its two: the other two are free for b without waiting for a revocation.
    let rejoin = r.beat(join("a"));
    assert_eq!(rejoin.assignment, Some(parts(&[0, 1])));
    let b = r.beat(poll("b", 3));
    assert_eq!(b.assignment, Some(parts(&[2, 3])));
}

#[test]
fn a_topic_made_after_the_member_subscribed_is_assigned_when_the_topics_change() {
    let mut r = rig(0);
    let a = r.beat(join("a"));
    assert_eq!(a.assignment, Some(parts(&[])));
    r.topics = topics(3);
    r.group
        .topics_changed(&r.topics, Millis(r.now), &mut Vec::new());
    let a = r.beat(poll("a", 2));
    assert_eq!(a.assignment, Some(parts(&[0, 1, 2])));
}

#[test]
fn a_deleted_topic_is_revoked_and_one_made_again_under_the_name_is_another_topic() {
    let mut r = rig(2);
    r.beat(join("a"));
    r.topics = Topics::new([]);
    r.group
        .topics_changed(&r.topics, Millis(r.now), &mut Vec::new());
    let a = r.beat(poll_owning("a", r.member("a").epoch, &[0, 1]));
    assert_eq!(a.assignment, Some(parts(&[])), "told to give it up");
    // A topic of the same name with another id: the old partitions are not the new ones.
    r.topics = Topics::new([TopicInfo {
        name: "t".into(),
        id: Uuid::from_u128(9),
        partitions: 2,
    }]);
    r.group
        .topics_changed(&r.topics, Millis(r.now), &mut Vec::new());
    let epoch = r.member("a").epoch;
    let a = r.beat(poll_owning("a", epoch, &[]));
    let got = a.assignment.unwrap_or_default();
    assert!(got.iter().all(|p| p.0 == Uuid::from_u128(9)), "{got:?}");
}

#[test]
fn a_topic_that_changes_what_a_member_is_to_have_moves_the_epoch_and_one_that_changes_nothing_does_not()
 {
    let mut r = rig(2);
    r.beat(join("a"));
    let before = r.view().epoch;
    // Nothing a member wants has changed: the same topics, again.
    r.group
        .topics_changed(&r.topics, Millis(r.now), &mut Vec::new());
    assert_eq!(r.view().epoch, before);
    // The topic grows: the member is to have more, and the group's epoch says so.
    r.topics = topics(4);
    r.group
        .topics_changed(&r.topics, Millis(r.now), &mut Vec::new());
    assert_eq!(r.view().epoch, before + 1);
    assert_eq!(r.view().assignment_epoch, before + 1);
}
