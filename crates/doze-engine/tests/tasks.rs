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

#[allow(clippy::unwrap_used)] // test code
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
    // A task that ran is not held by what needed it: its view shows no connection (D1's open item, conns=1).
    assert_eq!(e.get("migrate").unwrap().conns, 0);
    assert!(
        e.holders("migrate").is_empty(),
        "{:?}",
        e.holders("migrate")
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

/// A stop while a task runs asks it to stop (it exits 0 on SIGTERM: not killed), and the run ends as Go's does: the
/// caller hears "was stopped", the record and the journal say `failed`, its log run is closed; it is asleep, and what
/// it started is gone. The record written over the old `done` is the stop's (E2 audit 2, n1, n2, n4).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stopped_task_is_asked_to_stop_and_ends_failed() {
    let d = Dir::new("taskstop");
    let pidf = d.file("gc.pid");
    // It takes 300 ms to stop once asked: within the grace, so it is not killed (E2 audit 3, N4).
    let mut ts = task(&d, &["grandchild", &pidf.to_string_lossy()]);
    ts.env.insert("LINGER_MS".into(), "300".into());
    let e = Engine::builder("t", d.path())
        .service(Service::task("long", ts))
        .build()
        .unwrap();
    // It ran before: the old record must not survive a run cut short (E2 audit 2, M3).
    let old = doze_engine::applied::TaskRecord {
        result: "done".into(),
        why: "first run".into(),
        ..Default::default()
    };
    e.set_task_record("long", Some(old));
    let waking = {
        let e = e.clone();
        tokio::spawn(async move { e.run_task("long").await })
    };
    assert!(
        // Generous under a loaded machine (it once lost the race running beside every other test).
        eventually(Duration::from_secs(20), || pidf.exists()
            && pid_in(&pidf) > 0)
        .await
    );
    assert_eq!(
        e.get("long").unwrap().state,
        State::Booting,
        "booting while it runs"
    );
    e.stop("long", Cause::Command).await.unwrap();
    let woke = tokio::time::timeout(Duration::from_secs(20), waking)
        .await
        .unwrap()
        .unwrap();
    // Windows: a console process ended by the stop's CTRL_BREAK may exit STATUS_CONTROL_C_EXIT (0xC000013A) instead.
    let said = woke.unwrap_err().to_string();
    assert!(
        [0, -1_073_741_510]
            .iter()
            .any(|s| said == format!("task \"long\" was stopped (exited with status {s})")),
        "{said}"
    );
    assert_eq!(e.get("long").unwrap().state, State::Reaped);
    let rec = e.task_record("long").unwrap();
    assert_eq!(
        (rec.result.as_str(), rec.why.as_str()),
        ("failed", "asked for")
    );
    let want = [ev("wake", "long", "asked for"), ev("failed", "long", &said)];
    assert!(
        eventually(Duration::from_secs(5), || journal(&d) == want).await,
        "{:?}",
        journal(&d)
    );
    let runs = e.logs().runs("long").unwrap();
    assert!(runs.iter().all(|r| !r.live), "the run is closed: {runs:?}");
    let said_by_it = e.logs().tail("long", 0, 50).unwrap();
    assert!(
        said_by_it.iter().any(|l| l.text == "stopping"),
        "it was asked, and given the time to stop: {said_by_it:?}"
    );
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

/// A task's needs are held while it runs and released when it ends; a run over its `timeout` fails (E2 audit 2, M3).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_task_releases_its_needs_and_a_timeout_fails_it() {
    let d = Dir::new("taskneeds");
    let ran = d.file("ran");
    let mut slow = task(&d, &["ignore"]);
    slow.timeout = Some(Duration::from_millis(300));
    let e = Engine::builder("t", d.path())
        .service(svc("db", listener(&d, 0)))
        .service(
            Service::task("migrate", task(&d, &["append", &ran.to_string_lossy()])).needs("db"),
        )
        .service(Service::task("slow", slow))
        .build()
        .unwrap();
    e.wake("migrate", Cause::Command).await.unwrap();
    assert!(e.get("db").unwrap().state.running());
    assert_eq!(
        e.get("db").unwrap().conns,
        0,
        "released once the task ended"
    );
    let err = e
        .wake("slow", Cause::Command)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.starts_with("task \"slow\" failed: exit -1"), "{err}");
    assert_eq!(e.task_record("slow").unwrap().exit_code, -1);
    e.shutdown().await;
}

/// `run_task` joining a needed start that found the task up to date still runs it (E2 audit 2, M2).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn run_task_racing_a_needed_wake_still_runs() {
    let d = Dir::new("taskjoin");
    let ran = d.file("ran");
    std::fs::create_dir_all(d.path().join("in")).unwrap();
    // Big enough that hashing takes a while, so the forced call joins the needed start.
    std::fs::write(d.path().join("in/big.bin"), vec![7u8; 64 * 1024 * 1024]).unwrap();
    let e = Engine::builder("t", d.path())
        .service(Service::task(
            "m",
            task(&d, &["append", &ran.to_string_lossy()]).inputs(&["in/*"]),
        ))
        .build()
        .unwrap();
    e.wake("m", Cause::Command).await.unwrap();
    let needed = {
        let e = e.clone();
        tokio::spawn(async move { e.wake("m", Cause::Command).await })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    e.run_task("m").await.unwrap();
    let _ = needed.await;
    assert_eq!(lines(&ran), 2);
    assert_eq!(e.task_record("m").unwrap().why, "asked for");
    e.shutdown().await;
}

/// A stop while what a task needs is still starting: the task never ran, but its wake was journalled, so its end is
/// too (Go's `fail` on a cancelled `bootDeps`): `failed`, "was stopped while what it needs was starting"; asleep.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_task_stopped_while_its_needs_start_ends_failed() {
    let d = Dir::new("taskstopneeds");
    let ran = d.file("ran");
    let e = Engine::builder("t", d.path())
        .service(svc("slow", listener(&d, 60_000)))
        .service(Service::task("seed", task(&d, &["append", &ran.to_string_lossy()])).needs("slow"))
        .build()
        .unwrap();
    let waking = {
        let e = e.clone();
        tokio::spawn(async move { e.run_task("seed").await })
    };
    assert!(
        eventually(Duration::from_secs(5), || e.get("slow").unwrap().state
            == State::Booting)
        .await
    );
    e.stop("seed", Cause::Command).await.unwrap();
    let woke = tokio::time::timeout(Duration::from_secs(3), waking)
        .await
        .unwrap()
        .unwrap();
    let said = "task \"seed\" was stopped while what it needs was starting";
    assert_eq!(woke.unwrap_err().to_string(), said);
    assert_eq!(lines(&ran), 0, "it never ran");
    assert_eq!(e.get("seed").unwrap().state, State::Reaped);
    let seed: Vec<_> = journal(&d).into_iter().filter(|j| j.1 == "seed").collect();
    assert_eq!(
        seed,
        [ev("wake", "seed", "asked for"), ev("failed", "seed", said)]
    );
    e.shutdown().await;
}

/// A task whose command already exited 0, stopped while its run drains a pipe a child that left its group still holds:
/// the end is journalled as Go's is once the command has ended (`ready`, "exit 0"), the record says `done`, and the stop
/// leaves it asleep (E2 audit 3).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_task_stopped_after_its_command_ended_still_journals_its_end() {
    // On Windows the child's Job ends with the command, so no pipe outlives it and the run ends before the stop: the
    // case does not arise there.
    if std::env::consts::OS == "windows" {
        return;
    }
    let d = Dir::new("taskdrain");
    let pidf = d.file("left.pid");
    let e = Engine::builder("t", d.path())
        .service(Service::task(
            "mig",
            task(&d, &["leave", &pidf.to_string_lossy()]),
        ))
        .build()
        .unwrap();
    let waking = {
        let e = e.clone();
        tokio::spawn(async move { e.wake("mig", Cause::Command).await })
    };
    assert!(eventually(Duration::from_secs(5), || pid_in(&pidf) > 0).await);
    // The leader is gone; the run waits up to 1 s for the pipe the child holds.
    tokio::time::sleep(Duration::from_millis(300)).await;
    e.stop("mig", Cause::Command).await.unwrap();
    let woke = waking.await.unwrap();
    let left = pid_in(&pidf);
    let _ = doze_os::process::kill_recorded(left);
    assert!(woke.is_ok(), "{woke:?}");
    assert_eq!(
        journal(&d),
        [ev("wake", "mig", "first run"), ev("ready", "mig", "exit 0")]
    );
    assert_eq!(e.task_record("mig").unwrap().result, "done");
    assert_eq!(e.get("mig").unwrap().state, State::Reaped);
    e.shutdown().await;
}

/// A stop while a task is still deciding whether it has to run (its inputs are being read; here a FIFO no one writes
/// holds the read) ends the start at once: nothing runs, nothing is journalled or recorded, it is asleep, and the waiter
/// hears why (E2 audit 3, N1 and N10). Needs `mkfifo`, so it is skipped where there is none.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_task_stopped_while_it_decides_ends_at_once_and_says_why() {
    let d = Dir::new("taskdecide");
    let (fifo, ran) = (d.file("in.fifo"), d.file("ran"));
    // Git Bash on Windows has a `mkfifo` that makes a file a read does not wait on: no FIFO there.
    let made = std::process::Command::new("mkfifo").arg(&fifo).status();
    if !made.is_ok_and(|s| s.success()) || std::env::consts::OS == "windows" {
        return;
    }
    let mut ts = task(&d, &["append", &ran.to_string_lossy()]);
    ts.inputs = vec!["in.fifo".into()];
    let e = Engine::builder("t", d.path())
        .service(Service::task("seed", ts))
        .build()
        .unwrap();
    let waking = {
        let e = e.clone();
        tokio::spawn(async move { e.wake("seed", Cause::Command).await })
    };
    tokio::time::sleep(Duration::from_millis(400)).await;
    let stopped =
        tokio::time::timeout(Duration::from_secs(3), e.stop("seed", Cause::Command)).await;
    let woke = tokio::time::timeout(Duration::from_secs(3), waking).await;
    // Let every blocked read go, whatever happened (see `release_fifo_readers`).
    release_fifo_readers(&fifo);
    stopped
        .expect("the stop does not wait for the read")
        .unwrap();
    let woke = woke.expect("the start ends at once").unwrap();
    assert_eq!(
        woke.unwrap_err().to_string(),
        "seed was stopped while it was starting"
    );
    assert_eq!(lines(&ran), 0, "it never ran");
    assert!(journal(&d).is_empty(), "{:?}", journal(&d));
    assert_eq!(e.task_record("seed"), None);
    assert_eq!(e.get("seed").unwrap().state, State::Reaped);
    e.shutdown().await;
}

/// `doze task` runs the command once even when the state file cannot be read or written: the run is counted by the
/// machine, not seen in the record (E2 audit 4; Go's `setTaskRecord` logs and the call succeeds).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn run_task_runs_once_when_the_state_file_is_broken() {
    let d = Dir::new("taskbadstate");
    let ran = d.file("ran");
    std::fs::create_dir_all(d.path().join(".doze")).unwrap();
    std::fs::write(d.path().join(".doze/state.json"), "{").unwrap();
    let e = Engine::builder("t", d.path())
        .service(Service::task(
            "seed",
            task(&d, &["append", &ran.to_string_lossy()]),
        ))
        .build()
        .unwrap();
    let res = e.run_task("seed").await;
    assert!(res.is_ok(), "{res:?}");
    assert_eq!(lines(&ran), 1, "it ran once");
    e.shutdown().await;
}
