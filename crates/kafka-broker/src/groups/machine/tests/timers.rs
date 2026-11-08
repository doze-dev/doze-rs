//! Time: the initial delay, the rebalance deadline, sessions, and timers that are no longer current.

use kafka_wire::ResponseError as E;

use super::*;

#[test]
fn the_first_member_of_an_empty_group_waits_out_the_initial_delay_and_no_longer() {
    let mut g = Group::new(3_000);
    let t = g.join(join_request(None, &["range"]));
    g.advance(2_999);
    assert!(g.answer(t).is_none());
    g.advance(1);
    assert!(g.joined(t).error.is_none());
}

#[test]
fn a_member_arriving_during_the_delay_starts_another_window() {
    let mut g = Group::new(3_000);
    let first = g.join(join_request(None, &["range"]));
    g.advance(2_000);
    let second = g.join(join_request(None, &["range"]));
    g.advance(3_999); // 5.999 s: the first window ended at 3 s with a latecomer, so the next ends at 6 s
    assert!(g.answer(first).is_none() && g.answer(second).is_none());
    g.advance(1);
    let (a, b) = (g.joined(first), g.joined(second));
    assert_eq!((a.generation, b.generation), (1, 1));
    assert_eq!(a.members.len(), 2);
}

#[test]
fn the_delay_is_cut_short_by_the_rebalance_deadline() {
    let mut g = Group::new(3_000);
    let mut slow = join_request(None, &["range"]);
    slow.rebalance_ms = 4_000;
    let first = g.join(slow);
    g.advance(2_000);
    g.join(join_request(None, &["range"]));
    // Windows would end at 3 s and 6 s; the deadline is 4 s.
    g.advance(2_000);
    assert!(g.joined(first).error.is_none());
}

#[test]
fn a_running_group_does_not_wait_for_the_initial_delay() {
    let mut g = stable(1);
    let t = g.join(join_request(None, &["range"]));
    assert!(g.answer(t).is_none(), "m1 has to join again");
    let t1 = g.join(join_request(Some("m1"), &["range"]));
    assert!(g.joined(t1).error.is_none());
    assert!(g.joined(t).error.is_none());
}

#[test]
fn members_that_do_not_join_again_by_the_longest_rebalance_timeout_are_dropped() {
    let mut g = stable(3);
    let mut quick = join_request(Some("m1"), &["range"]);
    quick.rebalance_ms = 20_000;
    let t = g.join(quick);
    // The longest timeout of the members in the group at the start is the default 30 s of stable(). The two
    // that stay quiet keep their sessions alive by heartbeat, so that only the deadline can drop them.
    for _ in 0..5 {
        g.advance(5_000);
        for m in ["m2", "m3"] {
            let hb = g.heartbeat(m, 1);
            assert_eq!(g.heartbeat_answer(hb), Some(E::RebalanceInProgress));
        }
    }
    g.advance(4_999);
    assert!(g.answer(t).is_none());
    g.advance(1);
    let told = g.joined(t);
    assert_eq!(told.generation, 2);
    assert_eq!(g.machine.member_ids(), vec![id("m1")]);
    assert_eq!(told.leader, Some(id("m1")));
}

#[test]
fn a_silent_member_is_dropped_when_its_session_runs_out_and_the_others_rebalance() {
    let mut g = stable(2);
    g.advance(5_000);
    let t = g.heartbeat("m1", 1);
    assert_eq!(g.heartbeat_answer(t), None);
    g.advance(4_999); // 9.999 s since m2 was last heard from
    assert_eq!(g.machine.member_ids().len(), 2);
    g.advance(1);
    assert_eq!(g.machine.member_ids(), vec![id("m1")]);
    assert_eq!(g.machine.state(), State::PreparingRebalance);
    let t = g.heartbeat("m1", 1);
    assert_eq!(g.heartbeat_answer(t), Some(E::RebalanceInProgress));
    let joined = g.join(join_request(Some("m1"), &["range"]));
    assert_eq!(g.joined(joined).generation, 2);
}

#[test]
fn a_heartbeat_keeps_a_member_alive_and_a_sync_and_a_join_do_too() {
    let mut g = stable(1);
    for _ in 0..5 {
        g.advance(9_000);
        let t = g.heartbeat("m1", 1);
        assert_eq!(g.heartbeat_answer(t), None);
    }
    assert_eq!(g.machine.member_ids().len(), 1);
}

#[test]
fn a_member_waiting_for_the_group_is_not_expired_for_being_silent() {
    let mut g = stable(2);
    let t = g.join(join_request(Some("m1"), &["range"]));
    // m1's session is 10 s and its join waits for m2, who is alive, for up to 30 s: at 20 s it is still a member.
    for _ in 0..4 {
        g.advance(5_000);
        let hb = g.heartbeat("m2", 1);
        g.heartbeat_answer(hb);
    }
    assert!(g.machine.member_ids().contains(&id("m1")));
    assert!(g.answer(t).is_none());
}

#[test]
fn a_timer_that_was_set_again_does_nothing_when_the_old_one_arrives() {
    let mut g = stable(1);
    let (_, old) = g.timer(&TimerKind::Session(id("m1"))).unwrap();
    g.advance(5_000);
    let t = g.heartbeat("m1", 1);
    g.heartbeat_answer(t);
    g.fire(old);
    assert_eq!(g.machine.member_ids().len(), 1, "the old arming is stale");
    let (_, current) = g.timer(&TimerKind::Session(id("m1"))).unwrap();
    g.fire(current);
    assert!(g.machine.member_ids().is_empty(), "the current one is not");
}

#[test]
fn the_deadline_of_a_rebalance_that_has_finished_does_nothing() {
    let mut g = Group::new(100);
    let t = g.join(join_request(None, &["range"]));
    let (_, deadline) = g.timer(&TimerKind::Rebalance).unwrap();
    g.advance(500); // the delay ends and the rebalance completes
    g.joined(t);
    assert!(g.timer(&TimerKind::Rebalance).is_none());
    g.fire(deadline);
    assert_eq!(g.machine.generation(), 1);
    assert_eq!(g.machine.state(), State::CompletingRebalance);
}
