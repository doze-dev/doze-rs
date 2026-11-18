//! STR-50 to STR-62: members joining, keeping on, leaving and being fenced; statuses; endpoints.

use kafka_wire::ResponseError as E;

use super::super::{Endpoint, status};
use super::{World, active_count, beat, id, instance, join, ready_topics, refused};
use crate::groups::machine::TimerKind;

#[test]
fn a_member_at_a_greater_epoch_or_a_smaller_one_that_is_no_retry_is_fenced() {
    let mut w = World::new(ready_topics(2));
    let last = w.settle(&["a"]);
    let e = last["a"].epoch;
    let a = w.send(&beat("a", e + 1, None));
    assert_eq!(refused(&a).0, E::FencedMemberEpoch);
    assert!(refused(&a).1.contains(&format!("greater member epoch ({}) than the one known by the group coordinator ({e})", e + 1)));
    let a = w.send(&beat("a", e - 1, None));
    assert!(refused(&a).1.contains("smaller member epoch"));
    // The epoch before, with what it owns being what it holds: a retry, answered.
    let prev = w.group.members[&id("a")].held.prev_epoch;
    let a = w.send(&beat("a", prev, Some(w.held("a"))));
    assert!(prev == e || a.error.is_none(), "{a:?}");
}

#[test]
fn an_unknown_member_is_told_so_and_a_group_with_no_such_member_is_not_changed() {
    let mut w = World::new(ready_topics(2));
    w.settle(&["a"]);
    let epoch = w.group.epoch;
    let x = w.send(&beat("x", 3, None));
    assert_eq!(refused(&x), (E::UnknownMemberId, "Member x is not a member of group app."));
    assert_eq!(w.group.epoch, epoch);
}

#[test]
fn leaving_moves_the_epoch_and_the_last_member_leaves_the_targets_at_it() {
    let mut w = World::new(ready_topics(2));
    w.settle(&["a"]);
    let before = w.group.epoch;
    let left = w.send(&beat("a", -1, None));
    assert_eq!((left.epoch, left.acceptable_lag, left.heartbeat_ms), (-1, -1, 0));
    assert!(left.status.is_empty() && left.tasks.is_none());
    assert_eq!((w.group.epoch, w.group.assignment_epoch), (before + 1, before + 1));
    assert_eq!(w.group.state(), "Empty");
    assert!(!w.timers.contains_key(&TimerKind::Session(id("a"))));
}

#[test]
fn a_silent_member_is_fenced_when_its_session_runs_out() {
    let mut w = World::new(ready_topics(2));
    w.settle(&["a", "b"]);
    w.advance(44_000);
    let last = w.group.members[&id("a")].held.epoch;
    w.send(&beat("a", last, Some(w.held("a"))));
    w.advance(2_000);
    assert!(w.group.members.contains_key(&id("a")));
    assert!(!w.group.members.contains_key(&id("b")), "b went silent");
}

#[test]
fn a_member_that_does_not_revoke_in_its_rebalance_timeout_is_fenced() {
    let mut w = World::new(ready_topics(2));
    let mut a = join("a");
    a.rebalance_ms = 5_000;
    w.send(&a);
    w.advance(3_000);
    let first = w.send(&beat("a", 1, Some(w.held("a"))));
    assert_eq!(active_count(&w.held("a")), 4, "{first:?}");
    w.send(&join("b"));
    w.advance(1_000);
    // a is told to give two tasks up, and never says it has.
    w.send(&beat("a", first.epoch, Some(w.held("a"))));
    assert!(w.timers.contains_key(&TimerKind::Revoke(id("a"))));
    w.advance(5_001);
    assert!(!w.group.members.contains_key(&id("a")));
}

#[test]
fn a_static_member_that_leaves_for_now_keeps_its_place_and_its_instance_comes_back_under_a_new_id() {
    let mut w = World::new(ready_topics(2));
    let mut a = join("a");
    a.instance = instance("i-1");
    w.send(&a);
    w.advance(3_000);
    let mut keep = beat("a", 1, Some(w.held("a")));
    keep.instance = instance("i-1");
    let held = w.send(&keep);
    assert_eq!(active_count(&w.held("a")), 4);
    let mut back = join("a2");
    back.instance = instance("i-1");
    back.process = Some("process-a".into());
    let busy = w.send(&back);
    assert_eq!(
        refused(&busy),
        (E::UnreleasedInstanceId, "Static member a2 with instance id i-1 cannot join the group because the instance id is owned by a member.")
    );
    let mut away = beat("a", -2, None);
    away.instance = instance("i-1");
    let away = w.send(&away);
    assert_eq!(away.epoch, -2);
    let epoch = w.group.epoch;
    let again = w.send(&back);
    assert!(again.error.is_none(), "{again:?}");
    assert_eq!(w.group.epoch, epoch, "the same instance taking its place moves nothing");
    assert!(!w.group.members.contains_key(&id("a")));
    assert_eq!(w.held("a2"), held.tasks.unwrap_or_default());
    let mut stale = beat("a", 3, None);
    stale.instance = instance("i-1");
    assert_eq!(refused(&w.send(&stale)), (E::FencedInstanceId, "Static member a with instance id i-1 was fenced by member a2."));
}

#[test]
fn a_shutdown_request_is_told_to_every_member_until_the_group_is_empty() {
    let mut w = World::new(ready_topics(2));
    let last = w.settle(&["a", "b"]);
    let mut stop = beat("a", last["a"].epoch, Some(w.held("a")));
    stop.shutdown = true;
    let a = w.send(&stop);
    let words = "Streams group member a encountered a fatal error and requested a shutdown for the entire application.";
    assert!(a.status.contains(&(status::SHUTDOWN_APPLICATION, words.to_owned())));
    let b = w.send(&beat("b", last["b"].epoch, Some(w.held("b"))));
    assert!(b.status.contains(&(status::SHUTDOWN_APPLICATION, words.to_owned())));
    w.send(&beat("a", -1, None));
    w.send(&beat("b", -1, None));
    let c = w.send(&join("c"));
    assert!(c.status.iter().all(|s| s.0 != status::SHUTDOWN_APPLICATION));
}

#[test]
fn the_group_is_full_at_its_maximum_size() {
    let mut w = World::new(ready_topics(2));
    let mut a = join("a");
    a.timing.max_size = 1;
    w.send(&a);
    let mut b = join("b");
    b.timing.max_size = 1;
    assert_eq!(refused(&w.send(&b)), (E::GroupMaxSizeReached, "The streams group has reached its maximum capacity of 1 members."));
    // The member already in it may join again.
    assert!(w.send(&a).error.is_none());
}

#[test]
fn missing_client_tags_are_reported_to_version_one_clients_only() {
    let mut w = World::new(ready_topics(2));
    let mut a = join("a");
    a.timing.tags = vec!["zone".into(), "rack".into()];
    a.client_tags = Some([("rack".to_owned(), "r1".to_owned())].into_iter().collect());
    let v1 = w.send(&a);
    let words = "Missing required client tags for rack-aware standby assignment: [zone]. Configure them via 'client.tag.<tagKey>' in your Streams config.";
    assert!(v1.status.contains(&(status::MISSING_CLIENT_TAGS, words.to_owned())), "{:?}", v1.status);
    a.version = 0;
    let v0 = w.send(&a);
    assert!(v0.status.iter().all(|s| s.0 != status::MISSING_CLIENT_TAGS));
}

#[test]
fn each_member_with_an_endpoint_is_told_where_every_task_is_itself_last() {
    let mut w = World::new(ready_topics(2));
    for (m, port) in [("a", 7001), ("b", 7002)] {
        let mut j = join(m);
        j.endpoint = Some(Endpoint { host: "localhost".into(), port });
        w.send(&j);
    }
    w.advance(3_000);
    for _ in 0..6 {
        for m in ["a", "b"] {
            let e = w.group.members[&id(m)].held.epoch;
            w.send(&beat(m, e, Some(w.held(m))));
        }
        w.advance(1_000);
    }
    assert_eq!(w.group.state(), "Stable");
    let e = w.group.members[&id("a")].held.epoch;
    let a = w.send(&beat("a", e, Some(w.held("a"))));
    let mut stale = beat("a", a.epoch, Some(w.held("a")));
    stale.endpoint_epoch = a.endpoint_epoch - 1;
    let told = w.send(&stale).endpoints.expect("told, its endpoint epoch being stale");
    assert_eq!(told.iter().map(|e| e.endpoint.port).collect::<Vec<_>>(), [7002, 7001]);
    // Task p of a subtopology is partition p of each topic it reads.
    let mine = told.last().unwrap();
    let held = w.held("a");
    let in_partitions: Vec<i32> = held.active.iter().filter(|(s, _)| s == "0").map(|(_, p)| *p).collect();
    assert_eq!(mine.active.iter().find(|(t, _)| t == "in").map(|(_, p)| p.clone()).unwrap_or_default(), in_partitions);
    let mut fresh = beat("a", a.epoch, Some(w.held("a")));
    fresh.endpoint_epoch = a.endpoint_epoch;
    assert!(w.send(&fresh).endpoints.is_none());
}
