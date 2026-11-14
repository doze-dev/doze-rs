//! Go's `registry_test.go` and `subscribe_test.go`, and the exit decisions of `runtime.watch`, as table tests.

use super::*;
use crate::restart::Policy;

fn t0() -> Now {
    Now::current()
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[test]
fn connections_move_between_active_and_idle() {
    let (mut t, now) = (Table::new(), t0());
    t.mark_booting("db");
    t.mark_running("db", "127.0.0.1:1", 42, now);
    assert_eq!(t.state("db"), Some(State::Idle));
    assert_eq!(t.acquire("db"), 1);
    assert_eq!(t.state("db"), Some(State::Active));
    assert_eq!(t.acquire("db"), 2);
    assert_eq!(t.release("db", now), 1);
    assert_eq!(t.state("db"), Some(State::Active));
    assert_eq!(t.release("db", now), 0);
    assert_eq!(t.state("db"), Some(State::Idle));
    // Never below zero.
    assert_eq!(t.release("db", now), 0);
}

#[test]
fn reapable_only_after_the_idle_time_and_never_when_held_or_pinned() {
    let (mut t, now) = (Table::new(), t0());
    t.mark_running("a", "", 1, now);
    t.mark_running("b", "", 2, now);
    t.mark_running("c", "", 3, now);
    t.acquire("b");
    assert!(t.toggle_pin("c"));
    assert!(t.reapable(Some(secs(10)), now.after(secs(9))).is_empty());
    assert_eq!(t.reapable(Some(secs(10)), now.after(secs(10))), vec!["a"]);
    // No default and no time of its own: never.
    assert!(t.reapable(None, now.after(secs(1000))).is_empty());
}

#[test]
fn idle_time_is_per_instance() {
    let (mut t, now) = (Table::new(), t0());
    t.set_idle_policy("quick", false, Some(secs(1)));
    t.set_idle_policy("slow", false, Some(secs(100)));
    t.mark_running("quick", "", 1, now);
    t.mark_running("slow", "", 2, now);
    t.mark_running("dflt", "", 3, now);
    assert_eq!(t.reapable(None, now.after(secs(2))), vec!["quick"]);
    assert_eq!(
        t.reapable(Some(secs(50)), now.after(secs(60))),
        vec!["dflt", "quick"]
    );
}

#[test]
fn always_on_is_active_with_nothing_holding_it() {
    let (mut t, now) = (Table::new(), t0());
    t.set_idle_policy("worker", true, None);
    t.mark_running("worker", "", 7, now);
    assert_eq!(t.state("worker"), Some(State::Active));
    t.acquire("worker");
    t.release("worker", now);
    assert_eq!(t.state("worker"), Some(State::Active));
    assert!(t.reapable(Some(secs(1)), now.after(secs(100))).is_empty());
}

#[test]
fn reaped_clears_what_describes_a_process_and_keeps_the_rest() {
    let (mut t, now) = (Table::new(), t0());
    t.set_tainted("db", true);
    t.toggle_pin("db");
    t.mark_running("db", "sock", 9, now);
    t.acquire("db");
    t.set_healthy("db", true);
    t.mark_reaped("db");
    let v = t.get("db", now).unwrap_or_default();
    assert_eq!(
        (v.state, v.pid, v.conns, v.backend.as_str()),
        (State::Reaped, 0, 0, "")
    );
    assert_eq!((v.started_at, v.idle_since, v.healthy), (None, None, None));
    assert!(v.tainted && v.keep_awake, "taint and pin survive a stop");
    // A fresh start clears the error but not the taint.
    t.set_error("db", "boom");
    t.mark_booting("db");
    let v = t.get("db", now).unwrap_or_default();
    assert!(v.last_error.is_empty() && v.tainted);
}

#[test]
fn snapshot_is_sorted() {
    let (mut t, now) = (Table::new(), t0());
    for n in ["c", "a", "b"] {
        t.declare(n);
    }
    let names: Vec<_> = t.snapshot(now).into_iter().map(|i| i.name).collect();
    assert_eq!(names, ["a", "b", "c"]);
}

#[test]
fn transitions_ignore_connection_churn() {
    let (mut t, now) = (Table::new(), t0());
    t.mark_running("db", "", 1, now);
    assert_eq!(t.take_transitions(now).len(), 1);
    t.acquire("db");
    let moved = t.take_transitions(now);
    assert_eq!(moved.len(), 1, "idle to active is a transition");
    let v0 = t.version();
    t.acquire("db");
    t.release("db", now);
    assert!(
        t.take_transitions(now).is_empty(),
        "churn while active is not"
    );
    assert!(t.version() > v0, "but the version moves");
    t.toggle_pin("db");
    assert_eq!(t.take_transitions(now).len(), 1, "a pin is");
}

fn on_failure(max: u32) -> Restart {
    Restart {
        policy: Policy::OnFailure,
        backoff: secs(1),
        max_retries: max,
    }
}

fn crash(t: &mut Table, restart: Option<Restart>, now: Now) -> AfterExit {
    let exit = Exit {
        success: false,
        text: "exited with status 1",
        restart,
        supervised: true,
        shutting_down: false,
    };
    t.exited("api", exit, now)
}

#[test]
fn a_crash_loop_is_crashing_then_failed() {
    let (mut t, now) = (Table::new(), t0());
    for attempt in 1..=2 {
        t.mark_running("api", "", 5, now);
        let got = crash(&mut t, Some(on_failure(2)), now);
        let delay = backoff_for(secs(1), attempt);
        assert_eq!(
            got,
            AfterExit::Restart {
                delay,
                attempt,
                max: 2
            }
        );
        let v = t.get("api", now).unwrap_or_default();
        assert_eq!(v.state, State::Crashing);
        assert_eq!(v.restart_max, 2);
        assert_eq!(
            v.last_error,
            format!(
                "exited with status 1; restarting ({attempt}/2) in {}",
                go_duration(delay)
            )
        );
    }
    t.mark_running("api", "", 5, now);
    assert_eq!(
        crash(&mut t, Some(on_failure(2)), now),
        AfterExit::GaveUp { max: 2 }
    );
    let v = t.get("api", now).unwrap_or_default();
    assert_eq!(v.state, State::Failed);
    assert_eq!(
        v.last_error,
        "exited with status 1; gave up after 2 restarts"
    );
}

#[test]
fn a_long_life_resets_the_budget() {
    let (mut t, now) = (Table::new(), t0());
    t.mark_running("api", "", 5, now);
    let _ = crash(&mut t, Some(on_failure(1)), now);
    t.mark_running("api", "", 5, now);
    let later = now.after(STABILITY_WINDOW);
    assert!(matches!(
        crash(&mut t, Some(on_failure(1)), later),
        AfterExit::Restart { attempt: 1, .. }
    ));
}

#[test]
fn exits_without_a_restart_say_why() {
    let (mut t, now) = (Table::new(), t0());
    t.mark_running("api", "", 5, now);
    assert_eq!(crash(&mut t, None, now), AfterExit::Stays);
    assert_eq!(
        t.get("api", now).unwrap_or_default().last_error,
        "exited with status 1; not restarted"
    );
    let ok = Exit {
        success: true,
        text: "exited with status 0",
        restart: Some(on_failure(3)),
        supervised: true,
        shutting_down: false,
    };
    t.mark_running("api", "", 5, now);
    assert_eq!(t.exited("api", ok, now), AfterExit::Stays);
    let v = t.get("api", now).unwrap_or_default();
    assert_eq!(
        (v.state, v.last_error.as_str()),
        (State::Reaped, "exited with status 0 and was not restarted")
    );
    let backend = Exit {
        supervised: false,
        restart: None,
        ..ok
    };
    t.mark_running("db", "", 6, now);
    t.exited("db", backend, now);
    let v = t.get("db", now).unwrap_or_default();
    assert_eq!(v.state, State::Failed);
    assert_eq!(
        v.last_error,
        "exited unexpectedly; it wakes again on the next connection"
    );
}

#[test]
fn nothing_restarts_while_shutting_down() {
    let (mut t, now) = (Table::new(), t0());
    t.mark_running("api", "", 5, now);
    let exit = Exit {
        success: false,
        text: "exited with status 1",
        restart: Some(on_failure(5)),
        supervised: true,
        shutting_down: true,
    };
    assert_eq!(t.exited("api", exit, now), AfterExit::Stays);
    assert_eq!(t.state("api"), Some(State::Reaped));
}

#[test]
fn done_clears_the_error() {
    let (mut t, now) = (Table::new(), t0());
    t.set_error("migrate", "x");
    t.mark_done("migrate");
    let v = t.get("migrate", now).unwrap_or_default();
    assert_eq!((v.state, v.last_error.as_str()), (State::Done, ""));
}

#[test]
fn a_restart_that_is_booting_still_shows_its_try() {
    let (mut t, now) = (Table::new(), t0());
    t.mark_running("api", "x", 5, now);
    let _ = crash(&mut t, Some(on_failure(5)), now);
    t.mark_booting("api");
    let v = t.get("api", now).unwrap_or_default();
    assert_eq!(
        (v.state, v.restart_count, v.restart_max),
        (State::Booting, 1, 5)
    );
    assert_eq!((v.pid, v.backend.as_str()), (0, ""));
}
