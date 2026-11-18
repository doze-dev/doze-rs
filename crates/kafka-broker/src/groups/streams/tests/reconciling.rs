//! STR-40 to STR-45: a member's way to its target, revoking before it is given (Kafka's streams `CurrentAssignmentBuilder`).

use super::super::reconcile::{Held, Holders, Stage, reconcile};
use super::{World, beat, join, ready_topics, task, tasks};

#[test]
fn a_task_another_member_runs_is_given_only_once_that_member_has_let_go_of_it() {
    let mut w = World::new(ready_topics(2));
    let first = w.settle(&["a"]);
    assert_eq!(w.held("a").active.len(), 4);
    let epoch_a = first["a"].epoch;
    // b joins: the targets move two of a's tasks to b, but b gets nothing a still runs.
    let b = w.send(&join("b"));
    w.advance(1_000);
    let a = w.send(&beat("a", epoch_a, Some(w.held("a"))));
    assert_eq!(
        w.group.members[&super::id("a")].held.stage,
        Stage::Unrevoked
    );
    assert_eq!(a.epoch, epoch_a, "a stays at its epoch until it revokes");
    let given_to_a = a.tasks.clone().expect("a is told what it keeps");
    assert_eq!(given_to_a.active.len(), 2);
    let b2 = w.send(&beat("b", b.epoch, Some(w.held("b"))));
    assert!(w.held("b").active.is_empty(), "b waits: {b2:?}");
    assert_eq!(
        w.group.members[&super::id("b")].held.stage,
        Stage::Unreleased
    );
    // a says it now owns only what it keeps: its epoch moves on, and b is given the rest.
    let a2 = w.send(&beat("a", epoch_a, Some(given_to_a)));
    assert!(a2.epoch > epoch_a);
    let b3 = w.send(&beat("b", b2.epoch, Some(w.held("b"))));
    assert_eq!(w.held("b").active.len(), 2, "{b3:?}");
    assert_eq!(w.group.state(), "Stable");
}

#[test]
fn a_member_that_does_not_say_what_it_owns_is_not_taken_to_have_let_go() {
    let held = Held {
        epoch: 3,
        prev_epoch: 3,
        stage: Stage::Unrevoked,
        tasks: tasks(&[("0", 0)], &[]),
        revoking: tasks(&[("0", 1)], &[]),
        ..Held::default()
    };
    let target = tasks(&[("0", 0)], &[]);
    let holders = Holders::default();
    assert_eq!(reconcile(&held, "p", &target, 4, None, &holders), None);
    let still = tasks(&[("0", 0), ("0", 1)], &[]);
    assert_eq!(
        reconcile(&held, "p", &target, 4, Some(&still), &holders),
        None
    );
    let done = reconcile(&held, "p", &target, 4, Some(&target), &holders).unwrap();
    assert_eq!((done.epoch, done.stage), (4, Stage::Stable));
    assert!(done.revoking.is_empty());
}

#[test]
fn a_standby_to_become_active_is_kept_until_the_active_task_is_given() {
    let held = Held {
        epoch: 2,
        prev_epoch: 2,
        tasks: tasks(&[], &[("1", 0)]),
        ..Held::default()
    };
    let target = tasks(&[("1", 0)], &[]);
    // Another process still runs the task: the standby stays, the active task waits.
    let other = Held {
        tasks: tasks(&[("1", 0)], &[]),
        ..Held::default()
    };
    let mut holders = Holders::default();
    holders.add("q", &other);
    let next = reconcile(&held, "p", &target, 3, Some(&held.tasks), &holders).unwrap();
    assert_eq!(next.stage, Stage::Unreleased);
    assert!(next.tasks.standby.contains(&task("1", 0)) && next.tasks.active.is_empty());
    // Released: it is active now, and the standby is gone.
    let next = reconcile(
        &next,
        "p",
        &target,
        3,
        Some(&next.tasks),
        &Holders::default(),
    )
    .unwrap();
    assert_eq!(next.tasks, target);
    assert_eq!(next.given_at.get(&task("1", 0)), Some(&3));
}

#[test]
fn an_active_task_to_become_a_standby_on_the_same_member_is_a_standby_at_once() {
    let held = Held {
        epoch: 2,
        prev_epoch: 2,
        tasks: tasks(&[("1", 0)], &[]),
        given_at: [(task("1", 0), 2)].into_iter().collect(),
        ..Held::default()
    };
    let target = tasks(&[], &[("1", 0)]);
    let mut holders = Holders::default();
    holders.add("p", &held);
    let next = reconcile(&held, "p", &target, 3, Some(&held.tasks), &holders).unwrap();
    assert_eq!(next.stage, Stage::Unrevoked);
    assert!(next.tasks.standby.contains(&task("1", 0)));
    assert!(next.revoking.active.contains(&task("1", 0)));
    // The task being given up keeps the epoch it was given at, for commits made meanwhile.
    assert_eq!(next.given_at.get(&task("1", 0)), Some(&2));
}

#[test]
fn a_standby_is_not_given_where_its_process_already_runs_the_task() {
    let held = Held {
        epoch: 2,
        prev_epoch: 2,
        ..Held::default()
    };
    let target = tasks(&[], &[("1", 0)]);
    let sibling = Held {
        tasks: tasks(&[("1", 0)], &[]),
        ..Held::default()
    };
    let mut holders = Holders::default();
    holders.add("p", &sibling);
    let next = reconcile(&held, "p", &target, 3, Some(&held.tasks), &holders).unwrap();
    assert_eq!((next.stage, next.epoch), (Stage::Unreleased, 3));
    assert!(next.tasks.standby.is_empty());
    // A process of its own may hold it.
    let next = reconcile(&held, "elsewhere", &target, 3, Some(&held.tasks), &holders).unwrap();
    assert_eq!(next.tasks.standby.len(), 1);
}
