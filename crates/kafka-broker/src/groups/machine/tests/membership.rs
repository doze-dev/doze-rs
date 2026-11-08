//! Who may join, what they are told, and what a request that cannot be answered yet waits for.

use kafka_wire::ResponseError as E;

use super::*;

#[test]
fn a_client_with_an_id_the_broker_issued_is_seated_under_it_and_one_with_an_id_nobody_issued_is_unknown()
 {
    let mut g = Group::new(0);
    let mut req = join_request(Some("issued-earlier"), &["range"]);
    req.issued = true;
    let t = g.join(req);
    let joined = g.joined(t);
    assert!(joined.error.is_none());
    assert_eq!(joined.member_id, Some(id("issued-earlier")));
    // Without the flag the same id is a stranger's.
    let mut g = Group::new(0);
    let t = g.join(join_request(Some("stranger"), &["range"]));
    assert_eq!(g.joined(t).error, Some(E::UnknownMemberId));
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
fn a_join_that_would_not_seat_anyone_is_known_not_to_before_it_is_tried() {
    let mut g = Group::new(0);
    let t = g.join(join_request(None, &["range"]));
    g.joined(t);
    let seats = |g: &Group, req: &JoinRequest| g.machine.would_seat(req);
    assert!(seats(&g, &join_request(None, &["range"])), "a new member");
    assert!(
        seats(&g, &join_request(Some("m1"), &["range"])),
        "a member joining again"
    );
    assert!(
        !seats(&g, &join_request(Some("nobody"), &["range"])),
        "an unknown id"
    );
    assert!(
        !seats(&g, &join_request(None, &["sticky"])),
        "nothing in common"
    );
    let mut other = join_request(None, &["range"]);
    other.protocol_type = "connect".into();
    assert!(!seats(&g, &other), "another kind of group");
    let mut quick = join_request(None, &["range"]);
    quick.session_ms = 5;
    assert!(!seats(&g, &quick), "a session out of bounds");
    // Asking changed nothing.
    assert_eq!(g.machine.member_ids().len(), 1);
    assert_eq!(g.machine.generation(), 1);
}
#[test]
fn a_join_is_refused_for_a_session_out_of_bounds_before_anything_else_and_for_no_protocols_or_type_after()
 {
    for (session, want) in [
        (5_999, Some(E::InvalidSessionTimeout)),
        (6_000, None),
        (1_800_000, None),
        (1_800_001, Some(E::InvalidSessionTimeout)),
    ] {
        let mut req = join_request(None, &["range"]);
        req.session_ms = session;
        assert_eq!(refusal(&req), want, "session {session}");
        let mut g = Group::new(0);
        let t = g.join(req);
        let told = g.joined(t);
        assert_eq!(told.error, want, "session {session}");
        assert!(
            want.is_none() || told.member_id.is_none(),
            "no id for a refused session"
        );
    }
    let mut g = Group::new(0);
    // The session is judged before the protocols.
    let mut both = join_request(None, &[]);
    both.session_ms = 5;
    assert_eq!(refusal(&both), Some(E::InvalidSessionTimeout));
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
    assert!(
        g.forgotten.contains(&first),
        "and whoever waits for it is told so"
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

#[test]
fn only_a_consumer_group_has_its_members_subscriptions_read() {
    // A real subscription: version 0, one topic "t", no user data.
    let subscription = Bytes::from_static(&[0, 0, 0, 0, 0, 1, 0, 1, b't', 0, 0, 0, 0]);
    let described = |protocol_type: &str| {
        let mut g = Group::new(0);
        let mut req = join_request(None, &["range"]);
        req.protocol_type = protocol_type.into();
        req.protocols[0].metadata = subscription.clone();
        let t = g.join(req);
        g.joined(t);
        let t = g.send(Event::Describe);
        let Some(Answer::Describe(d)) = g.answer(t) else {
            panic!("a description");
        };
        d.subscribed
    };
    assert_eq!(described("consumer"), Some(["t".to_owned()].into()));
    assert_eq!(
        described("connect"),
        None,
        "the same bytes mean nothing to a Connect worker"
    );
}
