//! The lifecycle table under arbitrary event sequences and synthetic time, with the invariants PLAN.md names: a pinned
//! instance is never reapable, `done` only comes from a completed run, the restart budget is never exceeded, and what
//! describes a process exists only while one runs.

use doze_engine::State;
use doze_engine::lifecycle::{AfterExit, Exit, Now, Table};
use doze_engine::restart::{Policy, Restart};
use proptest::prelude::*;
use std::time::Duration;

const NAMES: [&str; 3] = ["a", "b", "c"];

#[derive(Debug, Clone)]
enum Ev {
    Boot(usize),
    Running(usize, bool),
    Acquire(usize),
    Release(usize),
    Pin(usize),
    Exit(usize, bool, u8),
    Stop(usize),
    Fail(usize),
    Done(usize),
    Tick(u16),
}

fn ev() -> impl Strategy<Value = Ev> {
    let n = 0usize..NAMES.len();
    prop_oneof![
        n.clone().prop_map(Ev::Boot),
        (n.clone(), any::<bool>()).prop_map(|(i, a)| Ev::Running(i, a)),
        n.clone().prop_map(Ev::Acquire),
        n.clone().prop_map(Ev::Release),
        n.clone().prop_map(Ev::Pin),
        (n.clone(), any::<bool>(), 0u8..4).prop_map(|(i, ok, p)| Ev::Exit(i, ok, p)),
        n.clone().prop_map(Ev::Stop),
        n.clone().prop_map(Ev::Fail),
        n.prop_map(Ev::Done),
        (0u16..2000).prop_map(Ev::Tick),
    ]
}

fn restart_of(p: u8) -> Option<Restart> {
    let policy = match p {
        0 => return None,
        1 => Policy::OnFailure,
        2 => Policy::Always,
        _ => Policy::No,
    };
    Some(Restart {
        policy,
        backoff: Duration::from_millis(100),
        max_retries: 2,
    })
}

struct World {
    t: Table,
    now: Now,
    done_ok: [bool; 3],
}

fn apply(w: &mut World, e: &Ev) {
    let name = |i: &usize| NAMES.get(*i).copied().unwrap_or("a");
    match e {
        Ev::Boot(i) => w.t.mark_booting(name(i)),
        Ev::Running(i, always) => {
            w.t.set_idle_policy(name(i), *always, None);
            w.t.mark_running(name(i), "127.0.0.1:1", 100, w.now);
        }
        Ev::Acquire(i) => drop(w.t.acquire(name(i))),
        Ev::Release(i) => drop(w.t.release(name(i), w.now)),
        Ev::Pin(i) => drop(w.t.toggle_pin(name(i))),
        Ev::Exit(i, ok, p) => {
            let exit = Exit {
                success: *ok,
                text: "exited",
                restart: restart_of(*p),
                supervised: true,
                shutting_down: false,
            };
            let after = w.t.exited(name(i), exit, w.now);
            if let AfterExit::Restart { attempt, max, .. } = after {
                assert!(attempt <= max, "the budget is never exceeded");
            }
        }
        Ev::Stop(i) => {
            w.t.reset_restart(name(i));
            w.t.mark_reaped(name(i));
        }
        Ev::Fail(i) => w.t.mark_failed(name(i), "boom"),
        Ev::Done(i) => {
            w.t.mark_done(name(i));
            if let Some(d) = w.done_ok.get_mut(*i) {
                *d = true;
            }
        }
        Ev::Tick(ms) => w.now = w.now.after(Duration::from_millis(u64::from(*ms))),
    }
}

fn check(w: &World) {
    let idle = Some(Duration::from_millis(500));
    let reapable = w.t.reapable(idle, w.now);
    for inst in w.t.snapshot(w.now) {
        let i = NAMES.iter().position(|n| *n == inst.name).unwrap_or(0);
        if inst.keep_awake {
            assert!(
                !reapable.contains(&inst.name),
                "a pinned instance is never reaped"
            );
        }
        if reapable.contains(&inst.name) {
            assert_eq!((inst.state, inst.conns), (State::Idle, 0));
        }
        if inst.state == State::Done {
            assert!(
                w.done_ok.get(i).copied().unwrap_or(false),
                "done only from a completed run"
            );
        }
        if inst.state == State::Idle {
            assert!(
                inst.conns == 0 && !inst.always_on,
                "idle means nothing holds it and it may sleep"
            );
        }
        if !inst.state.running() {
            assert!(
                inst.pid == 0 && inst.backend.is_empty() && inst.started_at.is_none(),
                "{inst:?}"
            );
        }
        if inst.state == State::Crashing {
            assert!(inst.restart_count <= inst.restart_max && inst.next_restart.is_some());
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2000, ..ProptestConfig::default() })]
    #[test]
    fn invariants_hold_for_any_sequence(evs in proptest::collection::vec(ev(), 1..80)) {
        let mut w = World { t: Table::new(), now: Now::current(), done_ok: [false; 3] };
        for e in &evs {
            apply(&mut w, e);
            check(&w);
        }
    }
}
