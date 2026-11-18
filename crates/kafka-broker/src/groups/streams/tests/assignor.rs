//! STR-30 to STR-36: the sticky task assignor, held to the cases of Kafka 4.4.0's `StickyTaskAssignorTest`.

use std::collections::{BTreeMap, BTreeSet};

use kafka_wire::ids::MemberId;

use super::super::assign::{Seat, SubTasks, assign};
use super::super::{Task, Tasks};
use super::{id, task, tasks};

fn subs(list: &[(&str, i32, bool)]) -> BTreeMap<String, SubTasks> {
    list.iter()
        .map(|(id, tasks, stateful)| {
            (
                (*id).to_owned(),
                SubTasks {
                    tasks: *tasks,
                    stateful: *stateful,
                },
            )
        })
        .collect()
}

struct M {
    id: MemberId,
    process: String,
    current: Tasks,
    offsets: BTreeMap<Task, i64>,
}

fn m(name: &str, process: &str, current: Tasks, offsets: &[(&str, i32, i64)]) -> M {
    M {
        id: id(name),
        process: process.to_owned(),
        current,
        offsets: offsets.iter().map(|(s, p, o)| (task(s, *p), *o)).collect(),
    }
}

fn run(
    subtopologies: &BTreeMap<String, SubTasks>,
    members: &[M],
    standbys: usize,
) -> BTreeMap<String, Tasks> {
    let seats: Vec<Seat<'_>> = members
        .iter()
        .map(|m| Seat {
            id: &m.id,
            process: &m.process,
            current: &m.current,
            offsets: &m.offsets,
        })
        .collect();
    assign(subtopologies, &seats, standbys)
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

fn none() -> Tasks {
    Tasks::default()
}

#[test]
fn a_new_process_gets_the_task_its_owner_has_beyond_its_quota() {
    let s = subs(&[("s", 3, false)]);
    let out = run(
        &s,
        &[
            m("m1", "p1", tasks(&[("s", 0), ("s", 2)], &[]), &[]),
            m("m2", "p2", tasks(&[("s", 1)], &[]), &[]),
            m("m3", "p3", none(), &[]),
        ],
        0,
    );
    assert_eq!(out["m1"], tasks(&[("s", 0)], &[]));
    assert_eq!(out["m2"], tasks(&[("s", 1)], &[]));
    assert_eq!(out["m3"], tasks(&[("s", 2)], &[]));
}

#[test]
fn a_process_with_two_members_takes_twice_the_load() {
    let s = subs(&[("s", 3, false)]);
    let out = run(
        &s,
        &[
            m("a", "p1", none(), &[]),
            m("b1", "p2", none(), &[]),
            m("b2", "p2", none(), &[]),
        ],
        0,
    );
    for member in ["a", "b1", "b2"] {
        assert_eq!(out[member].active.len(), 1, "{member}: {out:?}");
    }
}

#[test]
fn a_standby_goes_back_to_its_previous_owner_below_its_quota() {
    let s = subs(&[("s", 2, true)]);
    let out = run(
        &s,
        &[
            m("m1", "p1", tasks(&[("s", 0)], &[("s", 1)]), &[]),
            m("m2", "p2", tasks(&[("s", 1)], &[]), &[]),
            m("m3", "p3", none(), &[]),
        ],
        1,
    );
    assert_eq!(out["m1"], tasks(&[("s", 0)], &[("s", 1)]));
    assert_eq!(out["m2"], tasks(&[("s", 1)], &[]));
    assert_eq!(out["m3"], tasks(&[], &[("s", 0)]));
}

#[test]
fn current_owners_come_before_reported_offsets_and_the_most_caught_up_first() {
    let s = subs(&[("s", 4, true)]);
    let out = run(
        &s,
        &[
            m("m1", "p1", tasks(&[("s", 0)], &[]), &[]),
            m("m2", "p2", tasks(&[], &[("s", 0), ("s", 1)]), &[]),
            m(
                "m3",
                "p3",
                none(),
                &[("s", 0, 1_000_000), ("s", 1, 1_000_000), ("s", 2, 100)],
            ),
            m("m4", "p4", none(), &[("s", 2, 50)]),
        ],
        0,
    );
    assert_eq!(out["m1"], tasks(&[("s", 0)], &[]));
    assert_eq!(out["m2"], tasks(&[("s", 1)], &[]));
    assert_eq!(out["m3"], tasks(&[("s", 2)], &[]));
    assert_eq!(out["m4"], tasks(&[("s", 3)], &[]));
}

#[test]
fn on_a_cold_start_tasks_go_where_their_state_is() {
    let s = subs(&[("s", 4, true)]);
    let out = run(
        &s,
        &[
            m("m1", "p1", none(), &[("s", 0, 10), ("s", 1, 10)]),
            m("m2", "p2", none(), &[("s", 2, 10), ("s", 3, 10)]),
        ],
        0,
    );
    assert_eq!(out["m1"], tasks(&[("s", 0), ("s", 1)], &[]));
    assert_eq!(out["m2"], tasks(&[("s", 2), ("s", 3)], &[]));
}

#[test]
fn standbys_go_away_from_the_process_that_has_the_task() {
    let s = subs(&[("s", 2, true)]);
    let out = run(
        &s,
        &[
            m("m1", "p1", none(), &[("s", 0, 5)]),
            m("m2", "p2", none(), &[("s", 1, 5)]),
        ],
        1,
    );
    assert_eq!(out["m1"], tasks(&[("s", 0)], &[("s", 1)]));
    assert_eq!(out["m2"], tasks(&[("s", 1)], &[("s", 0)]));
}

/// Every active task once; standbys only of stateful subtopologies, never on a process that has the task, and as many
/// as there are processes to take them; a stateless subtopology has none.
#[test]
fn every_task_is_placed_once_and_no_process_holds_a_task_twice() {
    let s = subs(&[("0", 5, false), ("1", 5, true)]);
    let members: Vec<M> = ["a1", "a2", "b1", "c1"]
        .iter()
        .map(|n| m(n, &format!("p{}", &n[..1]), none(), &[]))
        .collect();
    for standbys in 0..=3 {
        let out = run(&s, &members, standbys);
        let mut actives: Vec<Task> = out
            .values()
            .flat_map(|t| t.active.iter().cloned())
            .collect();
        actives.sort();
        assert_eq!(actives.len(), 10);
        assert_eq!(actives.iter().collect::<BTreeSet<_>>().len(), 10);
        let mut by_process: BTreeMap<char, BTreeSet<Task>> = BTreeMap::new();
        for (member, t) in &out {
            let p = member.chars().next().unwrap();
            for x in t.active.iter().chain(&t.standby) {
                assert!(
                    by_process.entry(p).or_default().insert(x.clone()),
                    "{x:?} twice on {p} at {standbys}"
                );
            }
        }
        let standby_count: usize = out.values().map(|t| t.standby.len()).sum();
        // Three processes: a stateful task has at most two other processes for its standbys.
        assert_eq!(
            standby_count,
            5 * standbys.min(2),
            "standbys {standbys}: {out:?}"
        );
        assert!(
            out.values()
                .all(|t| t.standby.iter().all(|(sub, _)| sub == "1"))
        );
        assert!(out.values().all(|t| t.warmup.is_empty()));
    }
}

#[test]
fn a_member_keeps_what_it_runs_when_another_joins() {
    let s = subs(&[("s", 4, false)]);
    let first = run(&s, &[m("m1", "p1", none(), &[])], 0);
    assert_eq!(first["m1"].active.len(), 4);
    let second = run(
        &s,
        &[
            m("m1", "p1", first["m1"].clone(), &[]),
            m("m2", "p2", none(), &[]),
        ],
        0,
    );
    assert_eq!(second["m1"].active.len(), 2);
    assert!(second["m1"].active.is_subset(&first["m1"].active));
    assert_eq!(second["m2"].active.len(), 2);
}
