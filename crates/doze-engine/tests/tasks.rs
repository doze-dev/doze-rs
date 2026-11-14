//! Tasks as engine instances (Go's `runtime/tasks.go`): a task runs when it has to, ends `done`, records how it went in
//! `.doze/state.json`, and journals its wake (with why) and its end; a service waits for the tasks it needs.

mod common;

use common::*;
use doze_engine::events::Kind;
use doze_engine::tasks::TaskSpec;
use doze_engine::{Cause, Engine, Service, State};
use std::time::Duration;

fn task(d: &Dir, args: &[&str]) -> TaskSpec {
    TaskSpec::new(run(args), d.path())
}

fn journal(d: &Dir) -> Vec<(String, String, String)> {
    doze_journal::read(&d.path().join("run/journal.jsonl"))
        .unwrap()
        .into_iter()
        .map(|e| (e.kind, e.instance, e.detail))
        .collect()
}

fn ev(kind: &str, inst: &str, detail: &str) -> (String, String, String) {
    (kind.into(), inst.into(), detail.into())
}

/// Go's `runTask`, first run then up to date: `booting`, then `done`, the record written, `wake` (why) and `ready`
/// ("exit 0") journalled; a second wake runs nothing and journals nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_task_runs_once_and_is_done() {
    let d = Dir::new("taskonce");
    let ran = d.file("ran");
    let e = Engine::builder("t", d.path())
        .service(Service::task(
            "migrate",
            task(&d, &["append", &ran.to_string_lossy()]),
        ))
        .build()
        .unwrap();
    e.wake("migrate", Cause::Command).await.unwrap();
    assert_eq!(e.get("migrate").unwrap().state, State::Done);
    let rec = e.task_record("migrate").unwrap();
    assert_eq!((rec.result.as_str(), rec.exit_code), ("done", 0));
    assert_eq!(rec.why, "first run");
    assert!(
        rec.ran_at.ends_with('Z') && rec.ran_at.len() > 20,
        "{rec:?}"
    );
    assert!(d.path().join(".doze/state.json").exists());
    e.wake("migrate", Cause::Command).await.unwrap();
    assert_eq!(lines(&ran), 1, "up to date: not run again");
    assert_eq!(e.get("migrate").unwrap().state, State::Done);
    let j = journal(&d);
    assert_eq!(
        j,
        [
            ev("wake", "migrate", "first run"),
            ev("ready", "migrate", "exit 0")
        ]
    );
    e.shutdown().await;
}

/// A service needing a task waits for it, and the task says who needed it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_service_waits_for_the_task_it_needs() {
    let d = Dir::new("taskneed");
    let ran = d.file("ran");
    let e = Engine::builder("t", d.path())
        .service(Service::task(
            "migrate",
            task(&d, &["append", &ran.to_string_lossy()]),
        ))
        .service(svc("api", listener(&d, 0)).needs("migrate"))
        .build()
        .unwrap();
    e.wake("api", Cause::Command).await.unwrap();
    assert_eq!(lines(&ran), 1);
    assert_eq!(e.get("migrate").unwrap().state, State::Done);
    let rec = e.task_record("migrate").unwrap();
    assert_eq!(rec.why, "first run (needed by api)");
    assert!(
        e.holders("migrate").is_empty(),
        "a done task is held by nothing"
    );
    e.shutdown().await;
}

/// A failed run: the error quotes its last 10 lines, the record says `failed` with the exit code, the instance is
/// `failed`, and the next wake runs it again ("failed last time").
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_task_says_why_and_runs_again() {
    let d = Dir::new("taskfail");
    let e = Engine::builder("t", d.path())
        .service(Service::task("seed", task(&d, &["say", "12", "3", "line"])))
        .build()
        .unwrap();
    let err = e
        .wake("seed", Cause::Command)
        .await
        .unwrap_err()
        .to_string();
    let want: Vec<String> = (3..=12).map(|i| format!("line {i}")).collect();
    assert_eq!(
        err,
        format!("task \"seed\" failed: exit 3\n{}", want.join("\n"))
    );
    let rec = e.task_record("seed").unwrap();
    assert_eq!((rec.result.as_str(), rec.exit_code), ("failed", 3));
    assert_eq!(e.get("seed").unwrap().state, State::Failed);
    let _ = e.wake("seed", Cause::Command).await;
    assert_eq!(e.task_record("seed").unwrap().why, "failed last time");
    let kinds: Vec<String> = journal(&d).into_iter().map(|j| j.0).collect();
    assert_eq!(kinds, ["wake", "failed", "wake", "failed"]);
    e.shutdown().await;
}

/// A task with `inputs` runs again when they change; a manual one only when asked for (`doze task`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inputs_and_manual_decide_whether_it_runs() {
    let d = Dir::new("taskinputs");
    let ran = d.file("ran");
    let (ran_m, sql) = (d.file("ran_m"), d.path().join("db"));
    std::fs::create_dir_all(&sql).unwrap();
    std::fs::write(sql.join("1.sql"), "create").unwrap();
    let mut manual = task(&d, &["append", &ran_m.to_string_lossy()]);
    manual.manual = true;
    let e = Engine::builder("t", d.path())
        .service(Service::task(
            "migrate",
            task(&d, &["append", &ran.to_string_lossy()]).inputs(&["db/*.sql"]),
        ))
        .service(Service::task("reset", manual))
        .build()
        .unwrap();
    e.wake("migrate", Cause::Command).await.unwrap();
    e.wake("migrate", Cause::Command).await.unwrap();
    assert_eq!(lines(&ran), 1);
    std::fs::write(sql.join("2.sql"), "alter").unwrap();
    e.wake("migrate", Cause::Command).await.unwrap();
    assert_eq!(lines(&ran), 2);
    assert_eq!(e.task_record("migrate").unwrap().why, "inputs changed");
    e.wake("reset", Cause::Command).await.unwrap();
    assert_eq!(lines(&ran_m), 0, "manual: a wake does not run it");
    assert_eq!(e.get("reset").unwrap().state, State::Reaped);
    e.run_task("reset").await.unwrap();
    e.run_task("migrate").await.unwrap();
    assert_eq!((lines(&ran_m), lines(&ran)), (1, 3));
    assert_eq!(e.task_record("reset").unwrap().why, "asked for");
    assert!(e.run_task("nope").await.is_err());
    e.shutdown().await;
}

/// A stop while a task runs ends it: its process is gone, it is asleep, and it leaves no record, so it runs again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stopped_task_leaves_no_record() {
    let d = Dir::new("taskstop");
    let pidf = d.file("gc.pid");
    let e = Engine::builder("t", d.path())
        .service(Service::task(
            "long",
            task(&d, &["grandchild", &pidf.to_string_lossy()]),
        ))
        .build()
        .unwrap();
    let waking = {
        let e = e.clone();
        tokio::spawn(async move { e.wake("long", Cause::Command).await })
    };
    assert!(
        eventually(Duration::from_secs(5), || pidf.exists()
            && pid_in(&pidf) > 0)
        .await
    );
    e.stop("long", Cause::Command).await.unwrap();
    let woke = tokio::time::timeout(Duration::from_secs(3), waking)
        .await
        .unwrap()
        .unwrap();
    assert!(woke.is_err());
    assert_eq!(e.get("long").unwrap().state, State::Reaped);
    assert!(e.task_record("long").is_none());
    assert!(
        all_gone(&[pid_in(&pidf)]).await,
        "what it started goes with it"
    );
    e.shutdown().await;
}

/// The events of a task go to subscribers as to the journal.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_task_publishes_its_wake_and_ready() {
    let d = Dir::new("taskev");
    let e = Engine::builder("t", d.path())
        .service(Service::task("m", task(&d, &["exit", "0"])))
        .build()
        .unwrap();
    let mut rx = e.events();
    e.wake("m", Cause::Command).await.unwrap();
    let a = rx.recv().await.unwrap();
    let b = rx.recv().await.unwrap();
    assert_eq!((a.kind, b.kind), (Kind::Wake, Kind::Ready));
    e.shutdown().await;
}
