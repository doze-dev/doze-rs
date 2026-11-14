//! Supervision with real processes: crash loops, steps, and the "nothing left behind" gate after a stop and after a
//! crash loop (the host killed outright is in `leftovers.rs`).

mod common;

use common::*;
use doze_engine::events::Kind;
use doze_engine::{Cause, Engine, ProcessSpec, State, Step};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_takes_the_whole_tree() {
    let d = Dir::new("tree");
    let gc = d.file("gc.pid");
    let spec = ProcessSpec::new(run(&["grandchild", &gc.to_string_lossy()]), d.path()).port(1);
    let e = Engine::builder("t", d.path())
        .service(svc("web", spec))
        .build()
        .unwrap();
    e.wake("web", Cause::Command).await.unwrap();
    let (leader, grand) = (pid_of(&e, "web"), pid_in(&gc));
    assert!(leader > 0 && grand > 0 && doze_os::process::alive(grand));
    assert!(
        d.path().join("run/backend-web.pid").exists(),
        "the pid is recorded"
    );
    e.stop("web", Cause::Command).await.unwrap();
    assert!(
        all_gone(&[leader, grand]).await,
        "the grandchild went with its tree"
    );
    assert!(!d.path().join("run/backend-web.pid").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_crash_loop_restarts_then_gives_up_and_leaves_nothing() {
    let d = Dir::new("crash");
    // Alive past the 750 ms liveness check, then exits 3.
    let spec = ProcessSpec::new(run(&["exit", "3", "1000"]), d.path()).restart(quick_restart(2));
    let e = Engine::builder("t", d.path())
        .service(svc("worker", spec))
        .build()
        .unwrap();
    let mut events = e.events();
    e.wake("worker", Cause::Command).await.unwrap();
    let gave_up = eventually(Duration::from_secs(15), || {
        e.get("worker").is_some_and(|i| i.state == State::Failed)
    })
    .await;
    assert!(gave_up);
    let w = e.get("worker").unwrap();
    assert_eq!(
        w.last_error,
        "exited with status 3; gave up after 2 restarts"
    );
    let mut kinds = Vec::new();
    while let Ok(ev) = events.try_recv() {
        kinds.push((ev.kind, ev.cause, ev.detail));
    }
    let crashes = kinds.iter().filter(|(k, ..)| *k == Kind::Crash).count();
    assert_eq!(crashes, 3);
    assert!(
        kinds
            .iter()
            .any(|(k, c, _)| *k == Kind::Wake && *c == Cause::Restart)
    );
    assert!(
        kinds
            .iter()
            .any(|(k, _, d)| *k == Kind::GiveUp && d == "after 2 restarts")
    );
    assert!(
        kinds
            .iter()
            .any(|(k, _, d)| *k == Kind::Crash && d == "exited with status 3")
    );
    assert!(!d.path().join("run/backend-worker.pid").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_cancels_a_pending_restart() {
    let d = Dir::new("cancel");
    let mut r = quick_restart(5);
    r.backoff = Duration::from_millis(800);
    let spec = ProcessSpec::new(run(&["exit", "1", "900"]), d.path()).restart(r);
    let e = Engine::builder("t", d.path())
        .service(svc("w", spec))
        .build()
        .unwrap();
    e.wake("w", Cause::Command).await.unwrap();
    assert!(
        eventually(Duration::from_secs(5), || e
            .get("w")
            .is_some_and(|i| i.state == State::Crashing))
        .await
    );
    e.stop("w", Cause::Command).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let w = e.get("w").unwrap();
    assert_eq!(
        (w.state, w.restart_count),
        (State::Reaped, 0),
        "nothing respawned behind the stop"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn setup_runs_once_per_data_folder_and_a_failed_on_ready_runs_again_alone() {
    let d = Dir::new("steps");
    let (setup_log, flag) = (d.file("setup.log"), d.file("ready.flag"));
    let mut spec = listener(&d, 0);
    spec.setup = vec![Step::argv(run(&["append", &setup_log.to_string_lossy()]))];
    spec.on_ready = vec![Step::argv(run(&["check", &flag.to_string_lossy()]))];
    let e = Engine::builder("t", d.path())
        .service(svc("api", spec))
        .build()
        .unwrap();
    let err = e.wake("api", Cause::Command).await.unwrap_err().to_string();
    assert!(
        err.starts_with("on_ready for \"api\": on_ready step"),
        "{err}"
    );
    // Go's words for a step that failed (its `exec.ExitError`).
    assert!(err.ends_with("failed: exit status 1"), "{err}");
    let api = e.get("api").unwrap();
    assert_eq!(api.state, State::Failed);
    std::fs::write(&flag, "").unwrap();
    e.wake("api", Cause::Command).await.unwrap();
    assert!(e.get("api").unwrap().state.running());
    assert_eq!(
        lines(&d.file("starts")),
        1,
        "the command was not started again"
    );
    e.stop("api", Cause::Command).await.unwrap();
    e.wake("api", Cause::Command).await.unwrap();
    assert_eq!(lines(&setup_log), 1, "setup ran once for this data folder");
    let marker = d.path().join("clusters/api/.doze-setup");
    assert!(std::fs::read_to_string(marker).is_ok_and(|s| s.len() == 65));
    let pid = pid_of(&e, "api");
    e.shutdown().await;
    assert!(all_gone(&[pid]).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_start_that_fails_releases_its_needs_and_says_why() {
    let d = Dir::new("failstart");
    let bad = ProcessSpec::new(run(&["exit", "7"]), d.path());
    let e = Engine::builder("t", d.path())
        .service(svc("api", bad).needs("db"))
        .service(svc("db", listener(&d, 0)))
        .build()
        .unwrap();
    let err = e.wake("api", Cause::Command).await.unwrap_err().to_string();
    assert!(
        err.starts_with("api not ready: exited immediately"),
        "{err}"
    );
    assert_eq!(e.get("api").unwrap().state, State::Failed);
    assert_eq!(e.get("db").unwrap().conns, 0, "the need was released");
    let pid = pid_of(&e, "db");
    e.shutdown().await;
    assert!(all_gone(&[pid]).await);
}

/// Audit B1: a `log` check reads this start's lines only, never an earlier run's.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_log_check_waits_for_this_runs_line() {
    let d = Dir::new("logrun");
    let spec = listener(&d, 400).ready(doze_engine::Probe::Log("listening on".into()));
    let e = Engine::builder("t", d.path())
        .service(svc("api", spec))
        .build()
        .unwrap();
    e.wake("api", Cause::Command).await.unwrap();
    let first = pid_of(&e, "api");
    e.stop("api", Cause::Command).await.unwrap();
    let t = std::time::Instant::now();
    e.wake("api", Cause::Command).await.unwrap();
    assert!(
        t.elapsed() >= Duration::from_millis(400),
        "passed on the old run's line after {:?}",
        t.elapsed()
    );
    let second = pid_of(&e, "api");
    e.shutdown().await;
    assert!(all_gone(&[first, second]).await);
}
