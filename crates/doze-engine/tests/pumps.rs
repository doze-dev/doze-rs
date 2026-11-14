//! E2 audit 1, M1 and M5: what moving from a log file to pipes must keep. A failed start quotes the process's last
//! words (a line without its newline included); a plan of several tags each line with its process; a process that left
//! its group holding the pipe neither hangs a stop nor keeps writing into a closed run (W0).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod common;

use common::*;
use doze_engine::exec;
use doze_engine::logs::RunLog;
use doze_engine::{Cause, Engine, ProcessSpec};
use doze_module_proto::msg::run::{Plan, Process, Ready};
use std::time::{Duration, Instant};

/// M1: the quote is built after the unit is stopped, so the pump has recorded a last line that had no newline.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_start_quotes_its_last_words_without_a_newline() {
    let d = Dir::new("lastwords");
    let spec = ProcessSpec::new(run(&["partial", "FATAL-END", "3"]), d.path());
    let e = Engine::builder("t", d.path())
        .service(svc("w", spec))
        .build()
        .unwrap();
    let err = e.wake("w", Cause::Command).await.unwrap_err().to_string();
    assert!(err.contains("exited immediately"), "{err}");
    assert!(
        err.ends_with("FATAL-END"),
        "the quote has the last words: {err:?}"
    );
    let runs = e.logs().runs("w").unwrap();
    assert!(!runs[0].live, "the failed start's run is closed");
    e.shutdown().await;
}

fn helper(name: &str, port: u16) -> Process {
    Process {
        name: name.into(),
        program: HELPER.into(),
        args: vec!["listen".into(), "0".into()],
        env: [("PORT".to_string(), port.to_string())].into(),
        dir: None,
        after: vec![],
        ready: Some(Ready::Tcp(format!("127.0.0.1:{port}"))),
        hooks: vec![],
        stop: None,
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .unwrap()
        .port()
}

/// M5: in a plan of several processes each line says which one printed it (`o:<name>`, as Go); a step's last words
/// are in the log when it returns.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_plan_of_several_tags_each_line_with_its_process() {
    let d = Dir::new("tags");
    let log = RunLog::open(&d.path().join("logs"), "pair").unwrap();
    let plan = Plan {
        processes: vec![helper("a", free_port()), helper("b", free_port())],
    };
    let mut unit = exec::start(&plan, &log, Duration::from_secs(10), None)
        .await
        .unwrap();
    let pids = unit.pids();
    unit.stop(Duration::from_secs(5)).await;
    let (lines, _) = log
        .run()
        .store()
        .read("pair", doze_logstore::Cursor::default(), 100)
        .unwrap();
    let streams: Vec<String> = lines.iter().map(|l| l.stream.clone()).collect();
    assert!(streams.contains(&"o:a".to_string()), "{streams:?}");
    assert!(streams.contains(&"o:b".to_string()), "{streams:?}");
    assert!(all_gone(&pids).await);
}

/// W0 (E6): a process that left its group still holds the stdout pipe. The stop returns within its drain, the run is
/// closed, and nothing it prints after reaches the log. It is then killed here, by its exact pid.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_escaped_holder_of_the_pipe_neither_hangs_a_stop_nor_writes_after_it() {
    let d = Dir::new("escaped");
    let pidfile = d.file("escaped.pid");
    let spec = ProcessSpec::new(run(&["escape", &pidfile.to_string_lossy()]), d.path()).port(1);
    let e = Engine::builder("t", d.path())
        .service(svc("w", spec))
        .build()
        .unwrap();
    e.wake("w", Cause::Command).await.unwrap();
    let leader = pid_of(&e, "w");
    assert!(eventually(Duration::from_secs(5), || pid_in(&pidfile) != 0).await);
    let escaped = pid_in(&pidfile);
    assert!(
        eventually(Duration::from_secs(5), || e
            .logs()
            .tail("w", 0, 50)
            .unwrap()
            .iter()
            .any(|l| l.text.starts_with("escaped says")))
        .await,
        "the escaped child's lines reach the run while it is open"
    );
    let t = Instant::now();
    e.stop("w", Cause::Command).await.unwrap();
    assert!(t.elapsed() < Duration::from_secs(4), "{:?}", t.elapsed());
    assert!(!e.logs().runs("w").unwrap()[0].live, "the run is closed");
    let after = e.logs().tail("w", 0, 1000).unwrap().len();
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        e.logs().tail("w", 0, 1000).unwrap().len(),
        after,
        "nothing after the close"
    );
    let _ = doze_os::process::kill_recorded(escaped);
    e.shutdown().await;
    assert!(all_gone(&[leader, escaped]).await);
}
