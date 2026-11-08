//! Who may join, what they are told, and what a request that cannot be answered yet waits for.

use kafka_wire::ResponseError as E;

use super::*;

#[test]
fn a_client_that_joins_without_an_id_is_given_one_and_told_to_come_back_with_it() {
    let mut g = Group::new(0);
    let mut req = join_request(None, &["range"]);
    req.id_first = true;
    let t = g.join(req);
    let told = g.joined(t);
    assert_eq!(told.error, Some(E::MemberIdRequired));
    let given = told.member_id.unwrap();
    assert_eq!(told.generation, -1);
    assert_eq!(g.machine.state(), State::Empty, "nothing was joined yet");
    // Coming back with the id seats it.
    let t = g.join(join_request(Some(given.as_str()), &["range"]));
    let joined = g.joined(t);
    assert!(joined.error.is_none());
    assert_eq!(joined.member_id, Some(given));
}

#[test]
fn an_older_client_is_given_an_id_and_joins_in_one_step() {
    let mut g = Group::new(0);
    let t = g.join(join_request(None, &["range"]));
    let joined = g.joined(t);
    assert!(joined.error.is_none());
    assert_eq!(joined.generation, 1);
    assert_eq!(joined.leader, joined.member_id);
}

#[test]
fn an_id_that_was_never_issued_is_unknown() {
    let mut g = Group::new(0);
    let t = g.join(join_request(Some("nobody"), &["range"]));
    let told = g.joined(t);
    assert_eq!(told.error, Some(E::UnknownMemberId));
    assert_eq!(told.member_id, Some(id("nobody")));
}

#[test]
fn an_issued_id_is_not_spent_by_a_refused_join_and_is_forgotten_after_five_minutes() {
    let mut g = Group::new(0);
    let t = g.join(join_request(None, &["range"]));
    g.joined(t); // a member, so that the group has protocols to be consistent with
    let mut req = join_request(None, &["range"]);
    req.id_first = true;
    let issued = {
        let t = g.join(req.clone());
        g.joined(t).member_id.unwrap()
    };
    // A join with it that is refused (nothing in common with the group) does not use it up.
    let t = g.join(join_request(Some(issued.as_str()), &["sticky"]));
    assert_eq!(g.joined(t).error, Some(E::InconsistentGroupProtocol));
    let t = g.join(join_request(Some(issued.as_str()), &["range"]));
    let first = g.join(join_request(Some("m1"), &["range"])); // the member there was joins again, to let it through
    assert!(g.joined(t).error.is_none(), "still good");
    g.joined(first);
    // Another, unused for five minutes, is gone.
    let stale = {
        let t = g.join(req.clone());
        g.joined(t).member_id.unwrap()
    };
    g.advance(300_001);
    let t = g.join(req);
    g.joined(t); // issuing another is what purges
    let t = g.join(join_request(Some(stale.as_str()), &["range"]));
    assert_eq!(g.joined(t).error, Some(E::UnknownMemberId));
}

#[test]
fn the_session_bounds_are_checked_before_an_id_is_handed_out_and_protocols_after() {
    let mut g = Group::new(0);
    for (session, ok) in [
        (5_999, false),
        (6_000, true),
        (1_800_000, true),
        (1_800_001, false),
    ] {
        let mut req = join_request(None, &["range"]);
        req.id_first = true;
        req.session_ms = session;
        let t = g.join(req);
        let told = g.joined(t);
        let want = if ok {
            E::MemberIdRequired
        } else {
            E::InvalidSessionTimeout
        };
        assert_eq!(told.error, Some(want), "session {session}");
        if !ok {
            assert_eq!(told.member_id, None, "no id for a refused session");
        }
    }
    let t = g.join(join_request(None, &[]));
    assert_eq!(g.joined(t).error, Some(E::InconsistentGroupProtocol));
    let mut req = join_request(None, &["range"]);
    req.protocol_type = String::new();
    let t = g.join(req);
    assert_eq!(g.joined(t).error, Some(E::InconsistentGroupProtocol));
}

#[test]
fn a_member_with_another_protocol_type_or_none_in_common_is_refused() {
    let mut g = Group::new(0);
    let t = g.join(join_request(None, &["range"]));
    g.joined(t);
    let mut other_type = join_request(None, &["range"]);
    other_type.protocol_type = "connect".into();
    let t = g.join(other_type);
    assert_eq!(g.joined(t).error, Some(E::InconsistentGroupProtocol));
    let t = g.join(join_request(None, &["sticky"]));
    assert_eq!(g.joined(t).error, Some(E::InconsistentGroupProtocol));
    assert_eq!(g.machine.member_ids().len(), 1);
}

#[test]
fn members_who_join_together_share_a_generation_and_only_the_leader_is_told_the_members() {
    let mut g = Group::new(100);
    let a = g.join(join_request(None, &["range", "sticky"]));
    let b = g.join(join_request(None, &["sticky", "range"]));
    let c = g.join(join_request(None, &["sticky", "range"]));
    assert!(g.answer(a).is_none(), "held for the initial delay");
    g.advance(500);
    let (a, b, c) = (g.joined(a), g.joined(b), g.joined(c));
    assert!(
        [&a, &b, &c]
            .iter()
            .all(|j| j.generation == 1 && j.error.is_none())
    );
    // Two of three put sticky first, so it wins the vote.
    assert_eq!(a.protocol.as_deref(), Some("sticky"));
    assert_eq!(a.leader, Some(id("m1")), "the lowest id leads");
    assert_eq!(a.members.len(), 3);
    assert!(b.members.is_empty() && c.members.is_empty());
    assert!(
        a.members
            .iter()
            .all(|m| m.metadata.as_ref() == b"meta-sticky"),
        "each member's metadata is under the chosen protocol"
    );
}

#[test]
fn a_join_sent_again_replaces_the_one_waiting() {
    let mut g = Group::new(100);
    let first = g.join(join_request(None, &["range"]));
    let again = g.join(join_request(Some("m1"), &["range"]));
    g.advance(500);
    assert!(
        g.answer(first).is_none(),
        "the replaced request is not answered"
    );
    assert!(g.joined(again).error.is_none());
}

#[test]
fn a_follower_rejoining_a_stable_group_unchanged_is_told_at_once_and_a_leader_begins_a_rebalance() {
    let mut g = stable(2);
    let follower = g.join(join_request(Some("m2"), &["range"]));
    let told = g.joined(follower);
    assert_eq!((told.error, told.generation), (None, 1));
    assert_eq!(g.machine.state(), State::Stable);
    let leader = g.join(join_request(Some("m1"), &["range"]));
    assert!(
        g.answer(leader).is_none(),
        "the leader rejoining begins a rebalance"
    );
    assert_eq!(g.machine.state(), State::PreparingRebalance);
}

#[test]
fn a_new_member_or_changed_protocols_make_a_stable_group_rebalance() {
    let mut g = stable(2);
    let t = g.join(join_request(None, &["range"]));
    assert!(g.answer(t).is_none());
    assert_eq!(g.machine.state(), State::PreparingRebalance);
    let mut g = stable(2);
    let t = g.join(join_request(Some("m2"), &["range", "sticky"]));
    assert!(g.answer(t).is_none());
    assert_eq!(g.machine.state(), State::PreparingRebalance);
}

#[test]
fn heartbeats_answer_by_member_then_generation_then_whether_a_rebalance_is_under_way() {
    let mut g = stable(2);
    let t = g.heartbeat("ghost", 1);
    assert_eq!(g.heartbeat_answer(t), Some(E::UnknownMemberId));
    let t = g.heartbeat("m2", 7);
    assert_eq!(g.heartbeat_answer(t), Some(E::IllegalGeneration));
    let t = g.heartbeat("m2", 1);
    assert_eq!(g.heartbeat_answer(t), None);
    g.join(join_request(None, &["range"]));
    let t = g.heartbeat("m2", 1);
    assert_eq!(g.heartbeat_answer(t), Some(E::RebalanceInProgress));
}
