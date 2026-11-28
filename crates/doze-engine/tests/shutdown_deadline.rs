//! The shutdown has one deadline over all its waves. A dependant that will not stop holds up what it needs only until
//! then: at the deadline it is killed, what it needed is stopped at once with a short grace, and nothing is left running.

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
    // 2 s in order, then at most the 5 s the kills and the hurried stops get; well short of web's own 8 s grace plus
    // the waves under it.
    assert!(took < Duration::from_secs(20), "{took:?}");
    let journal = doze_journal::read(&d.path().join("run/journal.jsonl")).unwrap();
    let sleep = |name: &str| {
        journal
            .iter()
            .find(|ev| ev.kind == "sleep" && ev.instance == name)
            .map(|ev| ev.detail.clone())
    };
    let web = sleep("web").unwrap_or_default();
    assert!(
        web.contains("\"web\" was killed (SIGKILL): the shutdown ran out of time"),
        "{journal:?}"
    );
    // What web needed was stopped by its signal, not killed: a database gets its shutdown even when a dependant hangs.
    assert_eq!(sleep("api").as_deref(), Some(""), "{journal:?}");
    assert_eq!(sleep("db").as_deref(), Some(""), "{journal:?}");
}
