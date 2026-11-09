//! A group changing kind: what a parked request is told, the generations, and the size of the group.

use super::classic::{heartbeat_of, rejoin};
use super::*;

#[test]
fn a_syncgroup_waiting_for_the_leader_is_answered_rebalance_in_progress_when_the_group_is_upgraded()
{
    let mut r = Rig::new();
    // A second member joins a running group and the first joins again: one new generation, in which the leader has
    // not synced, so the follower's SyncGroup is parked.
    r.classic_member("a");
    let b = r.send(|t| Event::Join(t, join_request(Some("b"))));
    let a = r.send(|t| Event::Join(t, join_request(Some("a"))));
    let (Answer::Join(ja), Answer::Join(_)) = (r.answer(a), r.answer(b)) else {
        panic!("not joins")
    };
    let sync = r.send(|t| {
        Event::Sync(
            t,
            SyncRequest {
                member_id: id("b"),
                instance: None,
                generation: ja.generation,
                protocol_type: None,
                protocol: None,
                assignments: vec![],
            },
        )
    });
    assert!(r.answers.is_empty(), "parked");
    assert!(r.consumer("c", 0).error.is_none());
    let Answer::Sync(s) = r.answer(sync) else {
        panic!("not a sync")
    };
    assert_eq!(s.error, Some(E::RebalanceInProgress));
}

#[test]
fn a_classic_member_of_a_consumer_group_is_fenced_by_its_generation() {
    let mut r = Rig::new();
    let generation = r.classic_member("old");
    r.consumer("new", 0);
    let generation = rejoin(&mut r, "old").max(generation);
    assert_eq!(
        heartbeat_of(&mut r, "old", generation + 7),
        Some(E::IllegalGeneration)
    );
    assert_eq!(
        heartbeat_of(&mut r, "nobody", generation),
        Some(E::UnknownMemberId)
    );
}

#[test]
fn the_generation_after_a_downgrade_is_beyond_every_epoch_the_members_held() {
    let mut r = Rig::new();
    r.classic_member("old");
    r.consumer("new", 0);
    let held = rejoin(&mut r, "old");
    r.consumer("new", -1);
    assert!(r.view().is_none());
    let next = rejoin(&mut r, "old");
    assert!(next > held, "{next} after {held}");
}

#[test]
fn a_consumer_group_takes_a_thousand_members_and_no_more() {
    let mut r = Rig::new();
    for i in 0..1_000 {
        let a = r.consumer(&format!("m{i}"), 0);
        assert!(a.error.is_none(), "member {i}: {:?}", a.error);
    }
    let a = r.consumer("one-too-many", 0);
    assert_eq!(a.error.map(|(e, _)| e), Some(E::GroupMaxSizeReached));
    // One that is already a member may join again.
    assert!(r.consumer("m7", 0).error.is_none());
}
