//! A classic member of a consumer group joining and syncing again: what it keeps of what it held, what it is told when the
//! group has moved on, and the deadline it had to give something up.

use bytes::{BufMut, BytesMut};

use super::classic::{heartbeat_of, rejoin};
use super::*;

/// A version 1 subscription to `t` that says it still owns these partitions of it.
fn subscription_owning(owned: &[i32]) -> Bytes {
    let mut b = BytesMut::new();
    b.put_i16(1);
    b.put_i32(1);
    b.put_i16(1);
    b.put_slice(b"t");
    b.put_i32(-1);
    b.put_i32(i32::from(!owned.is_empty()));
    if !owned.is_empty() {
        b.put_i16(1);
        b.put_slice(b"t");
        b.put_i32(i32::try_from(owned.len()).unwrap());
        for p in owned {
            b.put_i32(*p);
        }
    }
    b.freeze()
}

fn join_owning(r: &mut Rig, name: &str, owned: &[i32]) {
    let mut req = join_request(Some(name));
    req.protocols[0].metadata = subscription_owning(owned);
    let t = r.send(|t| Event::Join(t, req));
    let Answer::Join(j) = r.answer(t) else {
        panic!("not a join")
    };
    assert!(j.error.is_none(), "{:?}", j.error);
}

fn member(r: &mut Rig, name: &str) -> crate::groups::consumer::MemberView {
    r.view()
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.id.as_str() == name)
        .unwrap()
}

/// "old" holds both partitions of `t`, a consumer has joined, and the group has moved far enough for "old" to be asked to
/// give one up: it has been told to join again.
fn old_asked_to_rejoin(r: &mut Rig) -> i32 {
    let generation = r.classic_member("old");
    r.consumer("new", 0);
    r.now += 1_500;
    let epoch = member(r, "new").epoch;
    r.consumer("new", epoch);
    assert_eq!(
        heartbeat_of(r, "old", generation),
        Some(E::RebalanceInProgress)
    );
    generation
}

#[test]
fn a_classic_member_joining_again_keeps_what_it_says_it_owns_and_has_given_up_the_rest() {
    let mut r = Rig::new();
    old_asked_to_rejoin(&mut r);
    let wanted = member(&mut r, "old").target;
    assert_eq!(wanted.len(), 1, "the consumer is to have the other");
    assert_eq!(member(&mut r, "old").assigned.len(), 2, "still holds both");
    // A cooperative consumer gives up one partition and says it owns the other.
    let keeps: Vec<i32> = wanted.iter().map(|(_, p)| *p).collect();
    join_owning(&mut r, "old", &keeps);
    assert_eq!(member(&mut r, "old").assigned, wanted);
}

#[test]
fn a_classic_syncgroup_after_the_group_has_moved_on_is_told_to_join_again() {
    let mut r = Rig::new();
    r.classic_member("old");
    // A consumer of another topic: the group moves on, and "old" has nothing to give up for it.
    let t = r.send(|t| {
        let mut hb = heartbeat("new", 0);
        hb.subscribed = Some(vec!["u".to_owned()]);
        Event::ConsumerHeartbeat(t, hb)
    });
    r.answer(t);
    let old = member(&mut r, "old");
    assert!(old.target.len() == 2 && old.assigned == old.target);
    let ask = |r: &mut Rig, generation: i32| {
        let t = r.send(|t| {
            Event::Sync(
                t,
                SyncRequest {
                    member_id: id("old"),
                    instance: None,
                    generation,
                    protocol_type: None,
                    protocol: None,
                    assignments: vec![],
                },
            )
        });
        let Answer::Sync(s) = r.answer(t) else {
            panic!("not a sync")
        };
        s.error
    };
    assert_eq!(ask(&mut r, old.epoch), Some(E::RebalanceInProgress));
    // Having joined again, it collects what it holds.
    let again = rejoin(&mut r, "old");
    assert_eq!(ask(&mut r, again), None);
}

#[test]
fn the_deadline_to_give_something_up_ends_when_the_member_joins_again_having_done_so() {
    let mut r = Rig::new();
    old_asked_to_rejoin(&mut r);
    let revoke = TimerKind::Revoke(id("old"));
    assert!(
        r.timers.contains_key(&revoke),
        "told to give up, with a deadline"
    );
    let keeps: Vec<i32> = member(&mut r, "old")
        .target
        .iter()
        .map(|(_, p)| *p)
        .collect();
    join_owning(&mut r, "old", &keeps);
    assert!(!r.timers.contains_key(&revoke), "it has done so");
}
