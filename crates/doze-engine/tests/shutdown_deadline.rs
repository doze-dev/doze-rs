//! The shutdown has one deadline over all its waves. A dependant that will not stop holds up what it needs only until
//! then: at the deadline it is killed, what it needed is stopped at once in a hurry, and nothing is left running.

mod common;

use common::*;
use doze_engine::{Cause, Engine, ProcessSpec};
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::unwrap_used)] // test code
async fn a_stuck_stop_at_the_top_of_a_chain_does_not_hold_the_shutdown_past_its_deadline() {
    let d = Dir::new("deadline");
    // web ignores SIGTERM (its stop would take the whole 8 s grace); it needs api, which needs db.
    let e = Engine::builder("t", d.path())
        .service(svc("db", listener(&d, 0)))
        .service(svc("api", listener(&d, 0)).needs("db"))
        .service(svc("web", ProcessSpec::new(run(&["ignore"]), d.path())).needs("api"))
        .build()
        .unwrap();
    e.wake("web", Cause::Command).await.unwrap();
    let pids = [pid_of(&e, "web"), pid_of(&e, "api"), pid_of(&e, "db")];
    assert!(pids.iter().all(|p| *p > 0), "{pids:?}");
    let began = Instant::now();
    e.shutdown_within(Duration::from_secs(2)).await;
    let took = began.elapsed();
    assert!(all_gone(&pids).await, "left running: {pids:?}");
    // 2 s in order, then at most 7 s for the hurried stops and the last kills; the bound is loose for a loaded machine
    // (the kill's words below, not the clock, show that the deadline cut web's stop short).
    assert!(took < Duration::from_secs(30), "{took:?}");
    // The journal line is written as the stop task ends, which may be a moment after the pids are gone.
    let path = d.path().join("run/journal.jsonl");
    let web_killed = || {
        doze_journal::read(&path).is_ok_and(|j| {
            j.iter().any(|ev| {
                ev.kind == "sleep"
                    && ev.instance == "web"
                    && ev
                        .detail
                        .contains("\"web\" was killed (SIGKILL): the shutdown ran out of time")
            })
        })
    };
    assert!(
        eventually(Duration::from_secs(10), web_killed).await,
        "{:?}",
        doze_journal::read(&path)
    );
    // What web needed was stopped after the deadline too (by its signal, or killed if the machine is too loaded for
    // the hurry's 2 s): each has its sleep.
    for name in ["api", "db"] {
        let slept = || {
            doze_journal::read(&path)
                .is_ok_and(|j| j.iter().any(|ev| ev.kind == "sleep" && ev.instance == name))
        };
        assert!(eventually(Duration::from_secs(10), slept).await, "{name}");
    }
}
