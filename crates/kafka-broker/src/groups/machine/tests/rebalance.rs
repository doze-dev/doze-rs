//! Syncing, leaving, and what a rebalance does to the requests that are waiting.

use kafka_wire::ResponseError as E;

use super::*;

/// Two members, joined and not yet synced: `m1` leads, `m2` follows.
fn joined_two() -> Group {
    let mut g = Group::new(100);
    let tokens: Vec<_> = (0..2)
        .map(|_| g.join(join_request(None, &["range"])))
        .collect();
    g.advance(500);
    for t in tokens {
        g.joined(t);
    }
    g
}

#[test]
fn the_leaders_assignment_is_handed_to_everyone_and_the_group_becomes_stable() {
    let mut g = joined_two();
    let follower = g.sync("m2", 1, &[]);
    assert!(g.answer(follower).is_none(), "waits for the leader");
    let leader = g.sync("m1", 1, &[("m1", "a1"), ("m2", "a2")]);
    let (l, f) = (g.synced(leader), g.synced(follower));
    assert_eq!(
        (l.assignment.as_ref(), f.assignment.as_ref()),
        (&b"a1"[..], &b"a2"[..])
    );
    assert_eq!(f.protocol.as_deref(), Some("range"));
    assert_eq!(f.protocol_type.as_deref(), Some("consumer"));
    assert_eq!(g.machine.state(), State::Stable);
    // A member who syncs later is told its assignment at once, and a leader syncing again changes nothing.
    let late = g.sync("m2", 1, &[]);
    assert_eq!(g.synced(late).assignment.as_ref(), b"a2");
    let again = g.sync("m1", 1, &[("m2", "other")]);
    g.synced(again);
    let check = g.sync("m2", 1, &[]);
    assert_eq!(g.synced(check).assignment.as_ref(), b"a2");
}

#[test]
fn a_member_with_no_assignment_gets_an_empty_one() {
    let mut g = joined_two();
    let leader = g.sync("m1", 1, &[("m1", "only-me")]);
    g.synced(leader);
    let t = g.sync("m2", 1, &[]);
    assert!(g.synced(t).assignment.is_empty());
}

#[test]
fn sync_answers_by_member_then_generation_then_protocol_then_state() {
    let mut g = joined_two();
    let t = g.sync("ghost", 1, &[]);
    assert_eq!(g.synced(t).error, Some(E::UnknownMemberId));
    let t = g.sync("m2", 9, &[]);
    assert_eq!(g.synced(t).error, Some(E::IllegalGeneration));
    // Version 5 names the protocol type and protocol the client thinks the group has.
    let mut wrong = SyncRequest {
        member_id: id("m2"),
        instance: None,
        generation: 1,
        protocol_type: Some("connect".into()),
        protocol: None,
        assignments: Vec::new(),
    };
    let t = g.send(|t| Event::Sync(t, wrong.clone()));
    assert_eq!(g.synced(t).error, Some(E::InconsistentGroupProtocol));
    wrong.protocol_type = Some("consumer".into());
    wrong.protocol = Some("sticky".into());
    let t = g.send(|t| Event::Sync(t, wrong.clone()));
    assert_eq!(g.synced(t).error, Some(E::InconsistentGroupProtocol));
    // While members are joining again, a sync is told to join again.
    g.join(join_request(None, &["range"]));
    let t = g.sync("m2", 1, &[]);
    assert_eq!(g.synced(t).error, Some(E::RebalanceInProgress));
}

#[test]
fn a_second_sync_from_a_waiting_member_replaces_the_first_which_is_told_it_is_unknown() {
    let mut g = joined_two();
    let first = g.sync("m2", 1, &[]);
    let second = g.sync("m2", 1, &[]);
    assert_eq!(g.synced(first).error, Some(E::UnknownMemberId));
    let leader = g.sync("m1", 1, &[("m2", "x")]);
    g.synced(leader);
    assert_eq!(g.synced(second).assignment.as_ref(), b"x");
}

#[test]
fn a_rebalance_tells_the_members_waiting_for_an_assignment_to_join_again() {
    let mut g = joined_two();
    let waiting = g.sync("m2", 1, &[]);
    g.join(join_request(None, &["range"]));
    assert_eq!(g.synced(waiting).error, Some(E::RebalanceInProgress));
}

#[test]
fn a_member_who_leaves_makes_the_others_join_again_and_the_generation_goes_up() {
    let mut g = stable(3);
    let t = g.leave(&["m3"]);
    assert!(matches!(g.answer(t), Some(Answer::Leave(r)) if r[0].error.is_none()));
    assert_eq!(g.machine.state(), State::PreparingRebalance);
    let a = g.join(join_request(Some("m1"), &["range"]));
    assert!(g.answer(a).is_none(), "m2 has not rejoined");
    let b = g.join(join_request(Some("m2"), &["range"]));
    let (a, b) = (g.joined(a), g.joined(b));
    assert_eq!((a.generation, b.generation), (2, 2));
    assert_eq!(a.members.len(), 2);
}

#[test]
fn when_the_leader_goes_the_lowest_remaining_id_leads_and_otherwise_the_leader_stays() {
    let mut g = stable(3);
    g.leave(&["m1"]);
    let (b, c) = (
        g.join(join_request(Some("m3"), &["range"])),
        g.join(join_request(Some("m2"), &["range"])),
    );
    assert_eq!(g.joined(b).leader, Some(id("m2")));
    g.joined(c);
    // m2 now leads; a follower leaving does not change that.
    let s = g.sync("m2", 2, &[]);
    g.synced(s);
    g.leave(&["m3"]);
    let t = g.join(join_request(Some("m2"), &["range"]));
    assert_eq!(g.joined(t).leader, Some(id("m2")));
}

#[test]
fn a_leader_that_is_still_a_member_keeps_leading_even_when_a_lower_id_joins() {
    let mut g = Group::named(100, &["b", "c", "a"]);
    let (b, c) = (
        g.join(join_request(None, &["range"])),
        g.join(join_request(None, &["range"])),
    );
    g.advance(500);
    assert_eq!(
        g.joined(b).leader,
        Some(id("b")),
        "the lowest id leads a new group"
    );
    g.joined(c);
    g.leave(&["b"]);
    let t = g.join(join_request(Some("c"), &["range"]));
    assert_eq!(g.joined(t).leader, Some(id("c")));
    let s = g.sync("c", 2, &[]);
    g.synced(s);
    // "a" sorts before "c", but c is still here.
    g.join(join_request(None, &["range"]));
    let t = g.join(join_request(Some("c"), &["range"]));
    g.advance(500);
    let told = g.joined(t);
    assert_eq!(told.leader, Some(id("c")));
    assert_eq!(told.members.len(), 2);
}

#[test]
fn the_last_member_leaving_empties_the_group_and_the_next_generation_is_two_on() {
    let mut g = stable(1);
    assert_eq!(g.machine.generation(), 1);
    g.leave(&["m1"]);
    assert_eq!(g.machine.state(), State::Empty);
    assert_eq!(
        g.machine.generation(),
        2,
        "emptying is a generation of its own"
    );
    assert!(g.machine.leader().is_none() && g.machine.member_ids().is_empty());
    let t = g.join(join_request(None, &["range"]));
    g.advance(500);
    assert_eq!(g.joined(t).generation, 3);
}

#[test]
fn leaving_names_each_member_and_an_unknown_one_disturbs_nothing() {
    let mut g = stable(2);
    let t = g.leave(&["ghost", "m2"]);
    let Some(Answer::Leave(results)) = g.answer(t) else {
        panic!("a leave answer");
    };
    assert_eq!(results[0].error, Some(E::UnknownMemberId));
    assert_eq!(results[0].member_id, Some(id("ghost")));
    assert_eq!(results[1].error, None);
    let mut g = stable(2);
    g.leave(&["ghost"]);
    assert_eq!(
        g.machine.state(),
        State::Stable,
        "no rebalance for a stranger"
    );
}

#[test]
fn a_member_who_leaves_while_its_join_waits_has_that_join_told_it_is_unknown() {
    let mut g = stable(2);
    let leading = g.join(join_request(Some("m1"), &["range"]));
    g.leave(&["m1"]);
    assert_eq!(g.joined(leading).error, Some(E::UnknownMemberId));
}

#[test]
fn the_first_member_to_join_an_empty_group_leads_it_whatever_its_id_sorts_as() {
    let mut g = Group::named(100, &["z", "a", "m"]);
    let tokens: Vec<_> = (0..3)
        .map(|_| g.join(join_request(None, &["range"])))
        .collect();
    g.advance(500);
    let answers: Vec<_> = tokens.into_iter().map(|t| g.joined(t)).collect();
    assert!(
        answers.iter().all(|a| a.leader == Some(id("z"))),
        "z came first"
    );
    assert_eq!(
        answers[0].members.len(),
        3,
        "and z is the one told the members"
    );
    assert!(answers[1].members.is_empty() && answers[2].members.is_empty());
}
