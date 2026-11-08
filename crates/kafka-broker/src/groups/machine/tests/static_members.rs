//! Static members: an instance id names a member across restarts.

use kafka_wire::ResponseError as E;

use super::*;

fn static_request(member: Option<&str>, inst: &str) -> JoinRequest {
    let mut r = join_request(member, &["range"]);
    r.instance = Some(instance(inst));
    r
}

/// A stable group of two static members, instances `i` and `j`: the group, the leader's id and the follower's.
fn with_statics() -> (Group, MemberId, MemberId) {
    let mut g = Group::new(100);
    let a = g.join(static_request(None, "i"));
    let b = g.join(static_request(None, "j"));
    g.advance(500);
    let (a, b) = (g.joined(a), g.joined(b));
    let (lead, follow) = (a.member_id.clone().unwrap(), b.member_id.unwrap());
    assert_eq!(a.leader.as_ref(), Some(&lead));
    let s = g.sync(
        lead.as_str(),
        1,
        &[(lead.as_str(), "lead"), (follow.as_str(), "mine")],
    );
    g.synced(s);
    (g, lead, follow)
}

#[test]
fn a_static_follower_that_restarts_takes_its_place_without_a_rebalance() {
    let (mut g, lead, old) = with_statics();
    let t = g.join(static_request(None, "j"));
    let told = g.joined(t);
    let new = told.member_id.unwrap();
    assert_ne!(new, old);
    assert_eq!(
        (told.error, told.generation),
        (None, 1),
        "no new generation"
    );
    assert_eq!(g.machine.state(), State::Stable);
    let members = g.machine.member_ids();
    assert!(members.contains(&new) && !members.contains(&old));
    assert_eq!(g.machine.leader(), Some(&lead));
    // It has the assignment the old one had.
    let s = g.sync(new.as_str(), 1, &[]);
    assert_eq!(g.synced(s).assignment.as_ref(), b"mine");
    // The old id is gone.
    let t = g.heartbeat(old.as_str(), 1);
    assert_eq!(g.heartbeat_answer(t), Some(E::UnknownMemberId));
}

#[test]
fn a_static_leader_that_restarts_makes_the_group_rebalance_and_the_new_incarnation_leads() {
    let (mut g, old, follower) = with_statics();
    let t = g.join(static_request(None, "i"));
    assert!(g.answer(t).is_none());
    assert_eq!(g.machine.state(), State::PreparingRebalance);
    assert_ne!(g.machine.leader(), Some(&old));
    let f = g.join(static_request(Some(follower.as_str()), "j"));
    let leader_told = g.joined(t);
    assert_eq!(g.joined(f).generation, 2);
    assert_eq!(leader_told.leader, leader_told.member_id);
}

#[test]
fn a_static_member_that_restarts_with_other_protocols_makes_the_group_rebalance() {
    let (mut g, _, follower) = with_statics();
    let mut r = static_request(None, "j");
    r.protocols = protocols(&["range", "sticky"]);
    let t = g.join(r);
    assert!(g.answer(t).is_none());
    assert_eq!(g.machine.state(), State::PreparingRebalance);
    assert!(!g.machine.member_ids().contains(&follower));
}

#[test]
fn the_old_incarnations_waiting_requests_are_told_they_were_fenced() {
    let mut g = Group::new(100);
    let first = g.join(static_request(None, "i"));
    let second = g.join(static_request(None, "i"));
    assert_eq!(g.joined(first).error, Some(E::FencedInstanceId));
    g.advance(500);
    assert!(g.joined(second).error.is_none());
}

#[test]
fn an_instance_held_by_another_member_fences_a_different_id_and_an_unheld_instance_is_unknown() {
    let (mut g, _, _) = with_statics();
    let t = g.join(static_request(Some("stranger"), "i"));
    assert_eq!(g.joined(t).error, Some(E::FencedInstanceId));
    let t = g.join(static_request(Some("stranger"), "elsewhere"));
    assert_eq!(g.joined(t).error, Some(E::UnknownMemberId));
}

#[test]
fn heartbeat_checks_the_instance_as_well_as_the_id() {
    let (mut g, member, other) = with_statics();
    let mut hb = HeartbeatRequest {
        member_id: member.clone(),
        instance: Some(instance("someone-else")),
        generation: 1,
    };
    let t = g.send(|t| Event::Heartbeat(t, hb.clone()));
    assert_eq!(g.heartbeat_answer(t), Some(E::UnknownMemberId));
    hb.instance = Some(instance("i"));
    let t = g.send(|t| Event::Heartbeat(t, hb.clone()));
    assert_eq!(g.heartbeat_answer(t), None);
    // An instance held by another member fences this id.
    hb.member_id = other;
    let t = g.send(|t| Event::Heartbeat(t, hb));
    assert_eq!(g.heartbeat_answer(t), Some(E::FencedInstanceId));
}

#[test]
fn a_static_member_can_leave_by_instance_alone_and_the_instance_is_then_free() {
    let (mut g, _, _) = with_statics();
    let t = g.send(|t| {
        Event::Leave(
            t,
            vec![LeaveTarget {
                member_id: None,
                instance: Some(instance("j")),
            }],
        )
    });
    let Some(Answer::Leave(r)) = g.answer(t) else {
        panic!("a leave answer");
    };
    assert_eq!(r[0].error, None);
    assert_eq!(g.machine.member_ids().len(), 1);
    let t = g.join(static_request(None, "j"));
    assert!(g.answer(t).is_none(), "a new member: a rebalance begins");
    assert_eq!(g.machine.member_ids().len(), 2);
}

#[test]
fn an_empty_instance_id_still_names_a_static_member() {
    let mut g = Group::new(0);
    let t = g.join(static_request(None, ""));
    let told = g.joined(t);
    assert!(
        told.error.is_none(),
        "joined in one step, not told to come back with an id"
    );
    assert_eq!(told.members[0].instance, Some(instance("")));
    // And it holds the instance: another member claiming it with another id is fenced.
    let t = g.join(static_request(Some("stranger"), ""));
    assert_eq!(g.joined(t).error, Some(E::FencedInstanceId));
}
