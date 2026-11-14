//! The catalogue rows the second audit found thinly proven (M4): W5, W6, W10, W12, P2, P8 and P10, each against real
//! processes and the journal. (N2's parent-death moments are in `leftovers.rs`.)

mod common;

use common::*;
use doze_engine::events::{Event, Kind};
use doze_engine::{Cause, Engine, Holder, ProcessSpec, State, Step};
use std::time::{Duration, Instant};
use tokio::sync::broadcast::Receiver;

fn drain(rx: &mut Receiver<Event>) -> Vec<Event> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        out.push(ev);
    }
    out
}

/// W5 (Go's `TestHolders`): running dependants sorted by name, then the pin, then client connections (the holds that
/// are not dependants).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn holders_are_dependants_sorted_then_the_pin_then_connections() {
    let d = Dir::new("holders");
    let e = Engine::builder("t", d.path())
        .service(svc("zeta", listener(&d, 0)).needs("db"))
        .service(svc("alpha", listener(&d, 0)).needs("db"))
        .service(svc("db", listener(&d, 0)))
        .build()
        .unwrap();
    e.wake("zeta", Cause::Command).await.unwrap();
    e.wake("alpha", Cause::Command).await.unwrap();
    assert!(e.toggle_pin("db").unwrap());
    e.acquire("db");
    e.acquire("db");
    assert_eq!(
        e.holders("db"),
        [
            Holder::Dependant("alpha".into()),
            Holder::Dependant("zeta".into()),
            Holder::Pin,
            Holder::Connections(2),
        ]
    );
    let pids: Vec<u32> = ["zeta", "alpha", "db"]
        .iter()
        .map(|n| pid_of(&e, n))
        .collect();
    e.stop("zeta", Cause::Command).await.unwrap();
    assert_eq!(
        e.holders("db"),
        [
            Holder::Dependant("alpha".into()),
            Holder::Pin,
            Holder::Connections(2)
        ],
        "a dependant that stopped no longer holds it"
    );
    e.shutdown().await;
    assert!(all_gone(&pids).await);
}

/// W12 (Go's `ToggleKeepAwake`): each toggle is journalled as `pin` `on` / `off`, caused by a command.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pin_toggle_is_journalled_on_then_off() {
    let d = Dir::new("pinjournal");
    let e = Engine::builder("t", d.path())
        .service(svc("db", listener(&d, 0)))
        .build()
        .unwrap();
    let mut rx = e.events();
    assert!(e.toggle_pin("db").unwrap());
    assert!(!e.toggle_pin("db").unwrap());
    let pins: Vec<(String, Cause)> = drain(&mut rx)
        .into_iter()
        .filter(|ev| ev.kind == Kind::Pin && ev.instance == "db")
        .map(|ev| (ev.detail, ev.cause))
        .collect();
    assert_eq!(
        pins,
        [
            ("on".to_string(), Cause::Command),
            ("off".to_string(), Cause::Command)
        ]
    );
}

/// W10: the reaper's stop is journalled as `sleep` with cause `idle`; a stop by command as `sleep` with cause `command`
/// (W6).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sleeps_are_journalled_with_their_cause() {
    let d = Dir::new("sleepcause");
    let e = Engine::builder("t", d.path())
        .idle_timeout(Duration::from_millis(600))
        .service(svc("idle", listener(&d, 0)))
        .service(svc("asked", listener(&d, 0)))
        .build()
        .unwrap();
    let mut rx = e.events();
    let _bg = e.background();
    e.wake("asked", Cause::Command).await.unwrap();
    e.acquire("asked");
    e.wake("idle", Cause::Command).await.unwrap();
    let pids = [pid_of(&e, "idle"), pid_of(&e, "asked")];
    assert!(
        eventually(Duration::from_secs(5), || e
            .get("idle")
            .is_some_and(|i| i.state == State::Reaped))
        .await
    );
    e.stop("asked", Cause::Command).await.unwrap();
    let sleeps: Vec<(String, Cause)> = drain(&mut rx)
        .into_iter()
        .filter(|ev| ev.kind == Kind::Sleep)
        .map(|ev| (ev.instance, ev.cause))
        .collect();
    assert_eq!(
        sleeps,
        [
            ("idle".to_string(), Cause::Idle),
            ("asked".to_string(), Cause::Command)
        ]
    );
    e.shutdown().await;
    assert!(all_gone(&pids).await);
}

/// W6's bounds, both at once: a `stop` step that hangs is cut at 10 s (its children with it) and the signals follow at
/// once; a step that succeeds but leaves the command running gives it 10 s to exit by itself, then the signals.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hanging_stop_step_is_cut_at_ten_seconds_and_the_patience_is_ten_seconds() {
    let d = Dir::new("stopbound");
    let left = d.file("left");
    let hang = Step::argv(run(&["slowchild", &left.to_string_lossy()]));
    let mut hangs = listener(&d, 0);
    hangs.stop = vec![hang.clone()];
    let mut stays = listener(&d, 0);
    stays.stop = vec![Step::argv(run(&[
        "append",
        &d.file("asked").to_string_lossy(),
    ]))];
    let e = Engine::builder("t", d.path())
        .service(svc("hangs", hangs))
        .service(svc("stays", stays))
        .build()
        .unwrap();
    let mut rx = e.events();
    let mut pids = Vec::new();
    let mut stops = tokio::task::JoinSet::new();
    for name in ["hangs", "stays"] {
        e.wake(name, Cause::Command).await.unwrap();
        pids.push(pid_of(&e, name));
        let e = e.clone();
        stops.spawn(async move {
            let t = Instant::now();
            e.stop(name, Cause::Command).await.unwrap();
            (name, t.elapsed())
        });
    }
    while let Some(r) = stops.join_next().await {
        let (name, took) = r.unwrap();
        assert!(
            took >= Duration::from_secs(10) && took < Duration::from_secs(16),
            "{name}: 10 s, then a prompt exit on the signal: {took:?}"
        );
    }
    assert_eq!(lines(&d.file("asked")), 1);
    let steps: Vec<String> = drain(&mut rx)
        .into_iter()
        .filter(|ev| ev.kind == Kind::Step)
        .map(|ev| ev.detail)
        .collect();
    let want = format!("stop: {}", hang.line);
    assert!(
        steps
            .iter()
            .any(|s| s.starts_with(&want) && s.contains("did not finish within 10s")),
        "{steps:?}"
    );
    pids.push(pid_in(&left));
    assert!(all_gone(&pids).await);
}

/// P2: a start that fails quotes this run's last 200 log lines, not an earlier run's.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_start_quotes_this_runs_last_200_lines() {
    let d = Dir::new("quote");
    let engine = |prefix: &str| {
        let spec = ProcessSpec::new(run(&["say", "250", "1", prefix]), d.path());
        Engine::builder("t", d.path())
            .service(svc("w", spec))
            .build()
            .unwrap()
    };
    let first = engine("first").wake("w", Cause::Command).await.unwrap_err();
    let quoted: Vec<String> = first
        .to_string()
        .lines()
        .filter(|l| l.starts_with("first "))
        .map(str::to_string)
        .collect();
    let want: Vec<String> = (51..=250).map(|i| format!("first {i}")).collect();
    assert_eq!(quoted, want, "{first}");
    // Audit 3, minor 10: a failed start leaves no record.
    assert!(!d.path().join("run/backend-w.pid").exists());
    let second = engine("second")
        .wake("w", Cause::Command)
        .await
        .unwrap_err();
    let text = second.to_string();
    assert!(text.contains("second 250"), "{text}");
    assert!(!text.contains("first "), "an earlier run quoted: {text}");
}

/// P8: a missing `env_file` is said before anything starts, but only after `setup` ran (so `setup` may write it).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn setup_may_write_the_env_file_the_start_then_reads() {
    let d = Dir::new("envorder");
    let env_file = d.file("app.env");
    let mut written = listener(&d, 0);
    written.env_file = Some(env_file.clone());
    written.setup = vec![Step::argv(run(&["append", &env_file.to_string_lossy()]))];
    let mut missing = listener(&d, 0);
    missing.env_file = Some(d.file("never.env"));
    let e = Engine::builder("t", d.path())
        .service(svc("written", written))
        .service(svc("missing", missing))
        .build()
        .unwrap();
    let err = e.wake("missing", Cause::Command).await.unwrap_err();
    assert!(err.to_string().contains("never.env"), "{err}");
    assert_eq!(lines(&d.file("starts")), 0, "nothing started");
    e.wake("written", Cause::Command).await.unwrap();
    assert_eq!(lines(&d.file("starts")), 1);
    let pid = pid_of(&e, "written");
    e.shutdown().await;
    assert!(all_gone(&[pid]).await);
}

/// P10: a failed `setup` step fails the start in Go's words, taints the instance, records no marker (so the next start
/// runs setup again), and starts nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_setup_taints_and_runs_again_next_time() {
    let d = Dir::new("setupfail");
    let ran = d.file("ran");
    let fail = Step::argv(run(&["exit", "3"]));
    let mut spec = listener(&d, 0);
    spec.setup = vec![
        Step::argv(run(&["append", &ran.to_string_lossy()])),
        fail.clone(),
    ];
    let e = Engine::builder("t", d.path())
        .service(svc("w", spec))
        .build()
        .unwrap();
    for n in 1..=2 {
        let err = e.wake("w", Cause::Command).await.unwrap_err();
        let want = format!("setup step \"{}\" failed: exit status 3", fail.line);
        assert!(err.to_string().contains(&want), "{err}");
        let w = e.get("w").unwrap();
        assert!(w.tainted && w.state == State::Failed, "{w:?}");
        assert!(w.last_error.contains(&want), "{w:?}");
        assert_eq!(
            lines(&ran),
            n,
            "setup runs again while it has not succeeded"
        );
    }
    assert_eq!(lines(&d.file("starts")), 0, "nothing started");
}

/// Go's `TestExtraEnvReachesSpecsTasksAndStepsAndKeepsTheInheritedEnvironment`: doze's own variables reach a
/// `setup` step and the process itself, on top of the inherited environment. (An exec readiness check runs in doze's
/// environment, as Go's `sh -c` did.)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dozes_own_variables_reach_steps_and_the_process() {
    let d = Dir::new("extraenv");
    let (steps, proc) = (d.file("steps"), d.file("proc"));
    let key = "DOZE_TEST_CA_FILE";
    let mut spec = ProcessSpec::new(run(&["envto", &proc.to_string_lossy(), key]), d.path());
    spec.setup = vec![Step::argv(run(&["envto", &steps.to_string_lossy(), key]))];
    spec.restart.max_retries = 0;
    let extra = [(key.to_string(), "/x/root.crt".to_string())];
    let e = Engine::builder("t", d.path())
        .extra_env(extra.into_iter().collect())
        .service(svc("w", spec))
        .build()
        .unwrap();
    // It exits at once (so the start fails): what it wrote first is the point.
    let _ = e.wake("w", Cause::Command).await;
    let read = |p: &std::path::Path| std::fs::read_to_string(p).unwrap_or_default();
    assert_eq!(read(&steps), "/x/root.crt\n");
    assert_eq!(read(&proc), "/x/root.crt\n");
    e.shutdown().await;
}
