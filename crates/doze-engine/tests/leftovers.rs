//! "Nothing left behind", by exact pid, at the moments the first audit found uncovered (B3, M11): the host killed while
//! a start waits for readiness and while a `setup` step runs, a crash loop's grandchildren, Linux's parent-death
//! signal on its own; and the steps and hooks around a run (`stop` steps, hooks, a restart pending at shutdown).

mod common;

use common::*;
use doze_engine::events::Kind;
use doze_engine::exec;
use doze_engine::logs::RunLog;
use doze_engine::{Cause, Engine, ProcessSpec, State, Step};
use doze_module_proto::msg::run::{Plan, Process, Ready};
use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};

/// Starts the stand-in daemon, waits until it reached `moment`, kills it outright; the recorded pid and a grandchild.
#[allow(clippy::unwrap_used)] // test code: a failure here is the test failing
fn kill_host_at(d: &Dir, moment: &str) -> Vec<u32> {
    let mut host = std::process::Command::new(HELPER)
        .args(["host", &d.path().to_string_lossy(), moment])
        .stdout(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(host.stdout.take().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    let pids: Vec<u32> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|p| p.parse().ok())
        .collect();
    assert_eq!(pids.len(), 2, "{moment}: {line:?}");
    assert!(pids.iter().all(|p| doze_os::process::alive(*p)), "{moment}");
    host.kill().unwrap();
    let _ = host.wait();
    pids
}

/// After the host died, the next engine's sweep leaves nothing, and no record.
#[allow(clippy::unwrap_used)] // test code
async fn next_engine_sweeps(d: &Dir, pids: &[u32]) {
    let next = Engine::builder("host", d.path()).build().unwrap();
    let _ = next.reconcile();
    assert!(all_gone(pids).await, "left behind: {pids:?}");
    let run = d.path().join("run");
    assert!(!run.join("backend-web.pid").exists());
    assert!(!run.join("backend-web.pids").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_host_killed_while_running() {
    let d = Dir::new("host-run");
    let pids = kill_host_at(&d, "running");
    if doze_os::process::leader_dies_with_parent() {
        // Linux's PDEATHSIG (Windows's Job object) alone, before any sweep: the leader goes.
        let leader = pids.first().copied().unwrap_or(0);
        assert!(
            eventually(Duration::from_secs(5), || !doze_os::process::alive(leader)).await,
            "the leader outlived its daemon"
        );
    }
    next_engine_sweeps(&d, &pids).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_host_killed_while_a_start_waits_for_readiness() {
    let d = Dir::new("host-ready");
    let pids = kill_host_at(&d, "readiness");
    next_engine_sweeps(&d, &pids).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_host_killed_during_a_setup_step() {
    let d = Dir::new("host-setup");
    let pids = kill_host_at(&d, "setup");
    next_engine_sweeps(&d, &pids).await;
}

/// N3: every run of a crash loop, and every grandchild each run started, is gone by pid once it gives up.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_crash_loop_leaves_no_process_by_pid() {
    let d = Dir::new("crashpids");
    let gcs = d.file("gcs");
    let run = run(&["crashchild", &gcs.to_string_lossy(), "3", "1000"]);
    let spec = ProcessSpec::new(run, d.path()).restart(quick_restart(2));
    let e = Engine::builder("t", d.path())
        .service(svc("worker", spec))
        .build()
        .unwrap();
    let mut leaders = Vec::new();
    e.wake("worker", Cause::Command).await.unwrap();
    let gave_up = eventually(Duration::from_secs(15), || {
        let p = pid_of(&e, "worker");
        if p > 0 && !leaders.contains(&p) {
            leaders.push(p);
        }
        e.get("worker").is_some_and(|i| i.state == State::Failed)
    })
    .await;
    assert!(gave_up);
    let grand: Vec<u32> = std::fs::read_to_string(&gcs)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect();
    assert_eq!(grand.len(), 3, "three runs, each with a grandchild");
    assert!(!leaders.is_empty());
    let all: Vec<u32> = leaders.iter().chain(&grand).copied().collect();
    assert!(all_gone(&all).await, "left behind: {all:?}");
}

/// W6: `stop` steps run in place of the signals, and the command exits because of them.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stop_steps_run_in_place_of_the_signals() {
    let d = Dir::new("stopsteps");
    let flag = d.file("stop.flag");
    let mut spec = listener(&d, 0).env("STOP_FILE", &flag.to_string_lossy());
    spec.stop = vec![Step::argv(run(&["append", &flag.to_string_lossy()]))];
    let e = Engine::builder("t", d.path())
        .service(svc("cache", spec))
        .build()
        .unwrap();
    let mut events = e.events();
    e.wake("cache", Cause::Command).await.unwrap();
    let pid = pid_of(&e, "cache");
    let t = Instant::now();
    e.stop("cache", Cause::Command).await.unwrap();
    assert!(
        t.elapsed() < Duration::from_secs(5),
        "the step stopped it, not the signals after the patience ran out: {:?}",
        t.elapsed()
    );
    assert_eq!(lines(&flag), 1, "the stop step ran once");
    assert!(all_gone(&[pid]).await);
    let mut steps = Vec::new();
    while let Ok(ev) = events.try_recv() {
        if ev.kind == Kind::Step {
            steps.push(ev.detail);
        }
    }
    assert!(
        steps.iter().any(|s| s.starts_with("stop: ")),
        "the journal has the stop step: {steps:?}"
    );
}

/// W7 (Go's `TestStopAllCancelsPendingRestart`): a restart pending at shutdown never happens.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_shutdown_cancels_a_pending_restart() {
    let d = Dir::new("pendingshut");
    let mut r = quick_restart(5);
    r.backoff = Duration::from_millis(800);
    let starts = d.file("starts");
    let spec = ProcessSpec::new(run(&["exit", "1", "900"]), d.path()).restart(r);
    let e = Engine::builder("t", d.path())
        .service(svc("w", spec.env("STARTS", &starts.to_string_lossy())))
        .build()
        .unwrap();
    e.wake("w", Cause::Command).await.unwrap();
    assert!(
        eventually(Duration::from_secs(5), || e
            .get("w")
            .is_some_and(|i| i.state == State::Crashing))
        .await
    );
    e.shutdown().await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let w = e.get("w").unwrap();
    assert!(!w.state.running(), "{:?}", w.state);
    assert!(
        e.wake("w", Cause::Command).await.is_err(),
        "nothing starts after the shutdown"
    );
}

fn helper_process(name: &str, listen: &str, hooks: Vec<Vec<String>>) -> Process {
    let mut env = std::collections::BTreeMap::new();
    let port = listen.rsplit(':').next().unwrap_or("0").to_string();
    env.insert("PORT".to_string(), port);
    Process {
        name: name.into(),
        program: HELPER.into(),
        args: vec!["listen".into()],
        env,
        dir: None,
        after: vec![],
        ready: Some(Ready::Tcp(listen.into())),
        hooks,
        stop: None,
    }
}

#[allow(clippy::unwrap_used)] // test code
fn free_addr() -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().to_string()
}

/// P3: hooks run after their process is ready, their output goes to the log, and a failing hook stops the plan and
/// says why in Go's words, quoting what it printed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hooks_run_after_readiness_and_a_failure_says_why() {
    let d = Dir::new("hooks");
    let log = RunLog::open(&d.path().join("run"), "web").unwrap();
    let ran = d.file("hook.ran");
    let ok_hook = run(&["append", &ran.to_string_lossy()]);
    let plan = Plan {
        processes: vec![helper_process("web", &free_addr(), vec![ok_hook])],
    };
    let mut unit = exec::start(&plan, &log, Duration::from_secs(10), None)
        .await
        .unwrap();
    assert_eq!(lines(&ran), 1, "the hook ran once");
    let pid = unit.pid();
    unit.stop(Duration::from_secs(5)).await;
    assert!(all_gone(&[pid]).await);

    let bad_hook = run(&["nosuchmode"]);
    let plan = Plan {
        processes: vec![helper_process("web", &free_addr(), vec![bad_hook.clone()])],
    };
    let err = exec::start(&plan, &log, Duration::from_secs(10), None)
        .await
        .err()
        .unwrap();
    let want = format!("web hook \"{}\": exit status 2: ", bad_hook.join(" "));
    assert!(err.message.starts_with(&want), "{:?}", err.message);
    assert!(
        err.message.contains("unknown mode"),
        "quotes its output: {:?}",
        err.message
    );
}

/// Audit 2, B1: a stop while a start waits for readiness wins at once (not when the readiness budget runs out): the
/// start fails, the instance is asleep (not failed, not running), and its process is gone.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_during_a_start_wins() {
    let d = Dir::new("stopstart");
    let mut spec = listener(&d, 30_000);
    spec.ready_timeout = Duration::from_secs(60);
    let e = Engine::builder("t", d.path())
        .service(svc("api", spec))
        .build()
        .unwrap();
    let waking = {
        let e = e.clone();
        tokio::spawn(async move { e.wake("api", Cause::Command).await })
    };
    assert!(eventually(Duration::from_secs(5), || lines(&d.file("starts")) == 1).await);
    let t = Instant::now();
    e.stop("api", Cause::Command).await.unwrap();
    assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    let woke = tokio::time::timeout(Duration::from_secs(3), waking)
        .await
        .unwrap()
        .unwrap();
    assert!(woke.is_err(), "{woke:?}");
    assert_eq!(e.get("api").unwrap().state, State::Reaped);
    let pid = pid_in(&d.file("starts"));
    assert!(pid > 0 && all_gone(&[pid]).await);
    assert!(!d.path().join("run/backend-api.pid").exists());
}

/// Audit 2, B2: a wake while the previous run is still being stopped waits for it: never two copies at once, and the
/// old one's record is not overwritten while it lives.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wake_during_a_stop_waits_for_the_old_run() {
    let d = Dir::new("wakestop");
    let e = Engine::builder("t", d.path())
        .service(svc("w", ProcessSpec::new(run(&["ignore"]), d.path())))
        .build()
        .unwrap();
    e.wake("w", Cause::Command).await.unwrap();
    let old = pid_of(&e, "w");
    let stopping = {
        let e = e.clone();
        tokio::spawn(async move { e.stop("w", Cause::Command).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    e.wake("w", Cause::Command).await.unwrap();
    assert!(
        !doze_os::process::alive(old),
        "the old run was gone before the new one started"
    );
    let new = pid_of(&e, "w");
    assert!(new > 0 && new != old);
    let recorded = std::fs::read_to_string(d.path().join("run/backend-w.pid")).unwrap_or_default();
    assert!(recorded.starts_with(&format!("{new}\n")), "{recorded:?}");
    let _ = stopping.await;
    e.shutdown().await;
    assert!(all_gone(&[old, new]).await);
}
