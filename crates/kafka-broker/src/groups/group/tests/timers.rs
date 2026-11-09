//! A group changing kind leaves no timer of the kind it was behind.

use super::classic::rejoin;
use super::*;

#[test]
fn an_upgrade_ends_the_rebalance_the_classic_group_was_waiting_on() {
    let mut r = Rig::new();
    r.classic_member("a");
    // b joins a running group: it waits for a to join again, with a deadline.
    r.send(|t| Event::Join(t, join_request(Some("b"))));
    assert!(r.timers.contains_key(&TimerKind::Rebalance), "waiting");
    r.consumer("c", 0);
    assert!(!r.timers.contains_key(&TimerKind::Rebalance));
    assert!(!r.timers.contains_key(&TimerKind::Hold));
    // Nothing the classic group had armed runs later against the consumer group.
    r.advance(120_000);
    assert!(r.view().is_some());
}

#[test]
fn a_member_upgraded_between_the_joins_and_the_leaders_sync_still_holds_what_the_generation_before_gave_it()
 {
    let mut r = Rig::new();
    r.classic_member("a");
    // b joins and a joins again: a new generation in which the leader has not synced, so nobody has an assignment yet.
    let b = r.send(|t| Event::Join(t, join_request(Some("b"))));
    let a = r.send(|t| Event::Join(t, join_request(Some("a"))));
    r.answer(a);
    r.answer(b);
    assert!(r.consumer("c", 0).error.is_none());
    let view = r.view().unwrap();
    let a = view.members.iter().find(|m| m.id.as_str() == "a").unwrap();
    assert_eq!(
        a.assigned.len(),
        2,
        "the partitions of the generation before"
    );
}

#[test]
fn a_downgrade_ends_the_deadlines_of_the_consumer_group() {
    let mut r = Rig::new();
    let generation = r.classic_member("old");
    r.consumer("new", 0);
    r.now += 1_500;
    let epoch = r
        .view()
        .unwrap()
        .members
        .iter()
        .find(|m| m.id.as_str() == "new")
        .unwrap()
        .epoch;
    r.consumer("new", epoch);
    super::classic::heartbeat_of(&mut r, "old", generation);
    let revoke = TimerKind::Revoke(id("old"));
    assert!(r.timers.contains_key(&revoke), "told to give something up");
    r.consumer("new", -1);
    assert!(r.view().is_none(), "classic again");
    assert!(!r.timers.contains_key(&revoke));
    // The old member rejoins at once and the group goes on as a classic one for a long time without losing it.
    let again = rejoin(&mut r, "old");
    r.advance(8_000);
    assert_eq!(super::classic::heartbeat_of(&mut r, "old", again), None);
}
