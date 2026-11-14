//! The port trap with real processes: an app that ignores the `$PORT` doze gave it is told where it listens instead,
//! and only when its check timed out while it still ran.
//!
//! Where the system cannot say what listens (no lsof or ss, as on Windows), there is no hint to check: each test then
//! returns early.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::*;
use doze_engine::exec;
use doze_engine::logs::RunLog;
use doze_engine::{Cause, Engine};
use doze_module_proto::msg::run::{Plan, Process, Ready};
use std::io::BufRead as _;
use std::time::Duration;

async fn cannot_say() -> bool {
    doze_engine::porttrap::listening(std::process::id())
        .await
        .is_none()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_app_listening_elsewhere_is_told_so() {
    if cannot_say().await {
        return;
    }
    let d = Dir::new("porttrap");
    let mut spec = listener(&d, 0).env("LISTEN_ON", "0");
    spec.ready_timeout = Duration::from_secs(2);
    let e = Engine::builder("t", d.path())
        .service(svc("web", spec))
        .build()
        .unwrap();
    let err = e.wake("web", Cause::Command).await.unwrap_err().to_string();
    // The helper says where it listens; the hint names that port and no other.
    let port = err
        .lines()
        .find_map(|l| l.trim().strip_prefix("listening on "))
        .unwrap_or_else(|| panic!("the quote has the helper's port: {err}"));
    let first = err.lines().next().unwrap_or_default();
    assert!(first.starts_with("web not ready: "), "{err}");
    assert!(
        first.contains(&format!(". web listens on :{port}, not on $PORT (")),
        "{err}"
    );
    assert!(
        first.contains("that doze gave it; start it with --port $PORT"),
        "{err}"
    );
    e.shutdown().await;
}

/// Stops the helper's listening child (and so the helper) however the test ends, and waits for the helper.
struct Stop(std::path::PathBuf, std::process::Child);

impl Drop for Stop {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.0, "stop");
        let _ = self.1.wait();
    }
}

/// Go's `TestListeningPortsFindsAChild`: a child that listens is found from its parent's pid, the case of an app
/// started through `sh -c` whose real server is a grandchild. Unlike Go, the root is a helper that listens on nothing
/// (not the test process), and the answer must be exactly the child's port: this process listens too, and lsof
/// without `-a` would list every process's ports.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_child_listening_is_found_from_its_parent() {
    if cannot_say().await {
        return;
    }
    let d = Dir::new("porttrap-child");
    let decoy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stop_file = d.file("stop");
    let parent = std::process::Command::new(HELPER)
        .arg("forklisten")
        .env("LISTEN_ON", "0")
        .env("STOP_FILE", &stop_file)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut parent = Stop(stop_file, parent);
    let mut line = String::new();
    // Read in place: the pipe stays open until the helper has gone, so the child's last words have somewhere to go.
    let out = parent.1.stdout.as_mut().unwrap();
    std::io::BufReader::new(out).read_line(&mut line).unwrap();
    let port: u16 = line
        .trim()
        .strip_prefix("listening on ")
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("helper said {line:?}"));
    assert_ne!(Some(port), decoy.local_addr().ok().map(|a| a.port()));
    let got = doze_engine::porttrap::listening(parent.1.id()).await;
    assert_eq!(got, Some(vec![port]));
    assert_eq!(doze_engine::porttrap::listening(0).await, None);
}

/// A helper given `$PORT` (where nothing listens) that listens elsewhere, with `ready`.
fn elsewhere(args: &[&str], ready: Ready) -> Plan {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .unwrap()
        .port();
    let env = [("PORT", port.to_string()), ("LISTEN_ON", "0".to_string())];
    Plan {
        processes: vec![Process {
            name: "w".into(),
            program: HELPER.into(),
            args: args.iter().map(|a| (*a).to_string()).collect(),
            env: env.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            dir: None,
            after: vec![],
            ready: Some(ready),
            hooks: vec![],
            stop: None,
        }],
    }
}

async fn start_err(tag: &str, plan: &Plan, budget: Duration) -> String {
    let d = Dir::new(tag);
    let log = RunLog::open(&d.path().join("logs"), "w").unwrap();
    match exec::start(plan, &log, budget, None).await {
        Ok(mut unit) => {
            unit.stop(Duration::from_secs(5)).await;
            panic!("{tag}: it started")
        }
        Err(e) => e.message,
    }
}

/// Not ready, but not a timeout: a check that cannot work says so, with no hint, though the app runs and listens
/// elsewhere.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_broken_check_gives_no_hint() {
    if cannot_say().await {
        return;
    }
    let plan = elsewhere(&["listen", "0"], Ready::LogLine("(".into()));
    let err = start_err("porttrap-broken", &plan, Duration::from_secs(5)).await;
    let first = err.lines().next().unwrap_or_default();
    assert!(
        first.starts_with("w not ready: invalid log_line regex"),
        "{err}"
    );
    assert!(!first.contains("listens on"), "{err}");
}

/// A timeout, but the process was gone by then (it exited while a slow check ran): there is no tree to look at, so no
/// hint.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_process_gone_at_the_timeout_gives_no_hint() {
    if cannot_say().await {
        return;
    }
    let slow_check = Ready::Exec(run(&["exit", "1", "1500"]));
    let plan = elsewhere(&["exit", "0", "200"], slow_check);
    let err = start_err("porttrap-gone", &plan, Duration::from_millis(500)).await;
    let first = err.lines().next().unwrap_or_default();
    assert_eq!(
        first, "w not ready: did not become ready within 500ms",
        "{err}"
    );
}
