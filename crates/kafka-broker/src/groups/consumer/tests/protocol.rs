//! The requests and what they are answered, in Kafka's words.

use kafka_wire::ResponseError as E;

use super::*;

fn refused(a: &HeartbeatAnswer) -> (E, &str) {
    let (e, m) = a.error.as_ref().expect("refused");
    (*e, m.as_str())
}

#[test]
fn the_first_member_of_a_new_group_is_at_epoch_2_and_is_given_everything() {
    let mut r = rig(3);
    let a = r.beat(join("a"));
    assert!(a.error.is_none());
    assert_eq!((a.epoch, a.assignment), (2, Some(parts(&[0, 1, 2]))));
    let v = r.view();
    assert_eq!((v.state, v.epoch, v.assignment_epoch), ("Stable", 2, 2));
}

#[test]
fn a_heartbeat_that_changes_nothing_is_answered_without_an_assignment() {
    let mut r = rig(3);
    r.beat(join("a"));
    r.advance(1_000);
    let again = r.beat(poll("a", 2));
    assert_eq!((again.epoch, again.assignment), (2, None));
}

#[test]
fn a_join_a_moment_after_the_last_targets_waits_for_a_heartbeat_past_the_interval() {
    let mut r = rig(2);
    r.beat(join("a"));
    r.advance(100);
    // b is answered at the epoch of the targets, with nothing yet, and the group is Assigning.
    let b = r.beat(join("b"));
    assert_eq!((b.epoch, b.assignment), (2, Some(parts(&[]))));
    let v = r.view();
    assert_eq!((v.state, v.epoch, v.assignment_epoch), ("Assigning", 3, 2));
    // Nothing happens without a heartbeat, however long it is.
    r.advance(2_000);
    assert_eq!(r.view().state, "Assigning");
    // The first heartbeat past the interval computes them: a is told to give up one.
    let a = r.beat(poll_owning("a", 2, &[0, 1]));
    assert_eq!((a.epoch, a.assignment), (2, Some(parts(&[0]))));
    assert_eq!(r.view().assignment_epoch, 3);
    assert_eq!(r.view().state, "Reconciling");
}

#[test]
fn a_leave_and_an_expiry_move_the_epoch_but_compute_nothing() {
    let mut r = rig(2);
    r.beat(join("a"));
    r.advance(1_500);
    r.beat(join("x"));
    assert_eq!(
        r.view().assignment_epoch,
        3,
        "a join after the interval computes at once"
    );
    r.advance(1_500);
    // x leaves: the epoch moves; the targets wait for the next heartbeat.
    let left = r.beat(Heartbeat {
        epoch: -1,
        ..poll("x", 3)
    });
    assert_eq!((left.error.is_none(), left.epoch), (true, -1));
    let v = r.view();
    assert_eq!((v.state, v.epoch, v.assignment_epoch), ("Assigning", 4, 3));
    // And a member that stops heartbeating goes when its session runs out, the same way.
    r.beat(join("y"));
    r.advance(SESSION + 1);
    assert!(
        r.view().members.iter().all(|m| m.id.as_str() != "y"),
        "y's session ran out"
    );
}

#[test]
fn what_a_client_gets_wrong_is_refused_in_kafkas_words() {
    let mut r = rig(2);
    r.beat(join("a"));
    let (code, message) = {
        let a = r.beat(poll("a", 7));
        (refused(&a).0, refused(&a).1.to_owned())
    };
    assert_eq!(code, E::FencedMemberEpoch);
    assert!(
        message.starts_with(
            "The consumer group member has a greater member epoch (7) than the one known"
        ),
        "{message}"
    );
    let a = r.beat(poll("nobody", 2));
    assert_eq!(
        refused(&a),
        (
            E::UnknownMemberId,
            "Member nobody is not a member of group g."
        )
    );
    let a = r.beat(Heartbeat {
        assignor: Some("sticky".into()),
        ..poll("a", 2)
    });
    assert_eq!(
        refused(&a),
        (
            E::UnsupportedAssignor,
            "ServerAssignor sticky is not supported. Supported assignors: uniform, range."
        )
    );
    let a = r.beat(Heartbeat {
        regex: Some("(".into()),
        ..poll("a", 2)
    });
    assert_eq!(refused(&a).0, E::InvalidRegularExpression);
    // A retry of the request that advanced the epoch is told again, not fenced.
    r.advance(1_500);
    r.beat(join("b"));
    r.advance(1_500);
    let a = r.beat(poll_owning("a", 2, &[0, 1]));
    let moved = (a.epoch, r.member("a").epoch);
    let retry = r.beat(poll_owning("a", 2, &[0]));
    assert!(retry.error.is_none(), "{moved:?}");
}

#[test]
fn a_client_without_an_id_is_given_one_by_version_0_and_refused_by_version_1() {
    let mut r = rig(1);
    let a = r.beat(Heartbeat {
        member_id: None,
        version: 0,
        ..join("")
    });
    assert_eq!(a.member_id, Some(id("made-1")));
    let a = r.beat(Heartbeat {
        member_id: None,
        version: 1,
        ..join("")
    });
    assert_eq!(refused(&a), (E::InvalidRequest, "a member id is required"));
}

#[test]
fn a_static_member_that_leaves_keeps_its_seat_and_a_new_incarnation_takes_it() {
    let mut r = rig(2);
    let with = |h: Heartbeat, i: &str| Heartbeat {
        instance: Some(instance(i)),
        ..h
    };
    r.beat(with(join("old"), "i1"));
    // Epoch -2: it goes away to come back. It holds its partitions at epoch -2.
    let gone = r.beat(Heartbeat {
        epoch: -2,
        ..with(poll("old", 2), "i1")
    });
    assert_eq!((gone.error.is_none(), gone.epoch), (true, -2));
    let m = r.member("old");
    assert_eq!((m.epoch, m.assigned), (-2, parts(&[0, 1])));
    assert_eq!(r.view().state, "Reconciling");
    // Another member joining past the interval is not given them.
    r.advance(1_500);
    let b = r.beat(join("b"));
    assert_eq!(b.assignment, Some(parts(&[])));
    // The restarted instance, under a new member id, takes the seat and what the old one had: it keeps its target
    // (one partition) and is told to give up the other, which b is then to have.
    let back = r.beat(with(join("new"), "i1"));
    assert_eq!(back.assignment, Some(parts(&[0])));
    assert_eq!(
        r.member("new").assigned,
        parts(&[0, 1]),
        "it still holds the one it was told to give up"
    );
    assert!(r.view().members.iter().all(|m| m.id.as_str() != "old"));
    // The old id is fenced (instance held by another member), and a stranger's instance is unknown.
    let fenced = r.beat(with(poll("old", 2), "i1"));
    assert_eq!(
        refused(&fenced),
        (
            E::FencedInstanceId,
            "Static member old with instance id i1 was fenced by member new."
        )
    );
    let unknown = r.beat(with(poll("b", 2), "nobody"));
    assert_eq!(
        refused(&unknown),
        (E::UnknownMemberId, "Instance id nobody is unknown.")
    );
    // A second live member cannot take an instance a live member holds.
    let taken = r.beat(with(join("third"), "i1"));
    assert_eq!(refused(&taken).0, E::UnreleasedInstanceId);
}

#[test]
fn a_static_member_that_does_not_return_is_removed_when_its_session_ends() {
    let mut r = rig(2);
    let with = |h: Heartbeat| Heartbeat {
        instance: Some(instance("i1")),
        ..h
    };
    r.beat(with(join("a")));
    r.beat(Heartbeat {
        epoch: -2,
        ..with(poll("a", 2))
    });
    r.advance(SESSION - 1);
    assert_eq!(r.view().members.len(), 1);
    r.advance(2);
    assert!(r.view().members.is_empty());
    assert_eq!(r.view().state, "Empty");
}
