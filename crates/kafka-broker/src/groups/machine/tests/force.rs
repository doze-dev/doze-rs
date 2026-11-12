//! The Lab's forced steps on a classic group: each is the step the group takes by itself for the same cause, taken now.

use super::*;
use crate::groups::note::{Cause, What};

fn force(g: &mut Group, f: Force) -> Forced {
    let t = g.send(|t| Event::Force(t, f));
    match g.answer(t) {
        Some(Answer::Forced(done)) => done,
        other => panic!("expected a forced answer, found {other:?}"),
    }
}

fn causes(g: &Group) -> Vec<Cause> {
    g.notes
        .iter()
        .filter_map(|n| match &n.what {
            What::Began { why, .. } => Some(why.cause),
            _ => None,
        })
        .collect()
}

#[test]
fn a_forced_expiry_leaves_the_group_as_the_session_timer_does() {
    let mut timed = stable(3);
    let mut forced = stable(3);
    timed.notes.clear();
    forced.notes.clear();
    // The timer path: m2 is silent while the others heartbeat, until its session runs out.
    for _ in 0..11 {
        timed.advance(1_000);
        timed.heartbeat("m1", 1);
        timed.heartbeat("m3", 1);
    }
    assert_eq!(force(&mut forced, Force::Expire(id("m2"))), Forced::Done);
    assert_eq!(forced.machine.state(), timed.machine.state());
    assert_eq!(forced.machine.member_ids(), timed.machine.member_ids());
    assert_eq!(forced.machine.member_ids(), [id("m1"), id("m3")]);
    assert_eq!(causes(&forced), [Cause::SessionExpired]);
    assert_eq!(causes(&timed), [Cause::SessionExpired]);
    // And the rest of the group finds out the way it does: its next heartbeat says to rejoin.
    let hb = forced.heartbeat("m1", 1);
    assert_eq!(forced.heartbeat_answer(hb), Some(ResponseError::RebalanceInProgress));
}

#[test]
fn a_member_with_a_join_waiting_is_answered_when_it_is_expired() {
    let mut g = stable(2);
    let parked = g.join(join_request(Some("m1"), &["range"]));
    assert!(g.answer(parked).is_none());
    assert_eq!(force(&mut g, Force::Expire(id("m1"))), Forced::Done);
    assert_eq!(g.joined(parked).error, Some(ResponseError::UnknownMemberId));
    assert_eq!(g.machine.member_ids(), [id("m2")]);
}

#[test]
fn expiring_the_last_member_empties_the_group() {
    let mut g = stable(1);
    assert_eq!(force(&mut g, Force::Expire(id("m1"))), Forced::Done);
    assert_eq!(g.machine.state(), State::Empty);
    assert_eq!(force(&mut g, Force::Expire(id("m1"))), Forced::NoSuchMember);
}

#[test]
fn a_forced_rebalance_makes_the_members_join_again_and_says_it_was_forced() {
    let mut g = stable(2);
    g.notes.clear();
    assert_eq!(force(&mut g, Force::Rebalance), Forced::Done);
    assert_eq!(g.machine.state(), State::PreparingRebalance);
    assert_eq!(causes(&g), [Cause::Forced]);
    assert_eq!(force(&mut g, Force::Rebalance), Forced::AlreadyRebalancing);
    let hb = g.heartbeat("m2", 1);
    assert_eq!(g.heartbeat_answer(hb), Some(ResponseError::RebalanceInProgress));
    let a = g.join(join_request(Some("m1"), &["range"]));
    let b = g.join(join_request(Some("m2"), &["range"]));
    assert_eq!((g.joined(a).generation, g.joined(b).generation), (2, 2));
    assert_eq!(g.machine.state(), State::CompletingRebalance);
    assert_eq!(g.machine.member_ids(), [id("m1"), id("m2")]);
}

#[test]
fn a_forced_rebalance_of_an_empty_group_is_refused() {
    let mut g = Group::new(0);
    assert_eq!(force(&mut g, Force::Rebalance), Forced::Empty);
}

#[test]
fn a_forced_rebalance_parks_a_sync_waiting_on_the_leader_and_tells_it_to_join_again() {
    let mut g = Group::new(100);
    let a = g.join(join_request(None, &["range"]));
    let b = g.join(join_request(None, &["range"]));
    g.advance(500);
    let (ja, jb) = (g.joined(a), g.joined(b));
    assert_eq!(g.machine.state(), State::CompletingRebalance);
    let follower = if ja.leader == ja.member_id { jb.member_id } else { ja.member_id }.unwrap();
    let waiting = g.sync(follower.as_str(), ja.generation, &[]);
    assert!(g.answer(waiting).is_none());
    assert_eq!(force(&mut g, Force::Rebalance), Forced::Done);
    assert_eq!(g.synced(waiting).error, Some(ResponseError::RebalanceInProgress));
}
