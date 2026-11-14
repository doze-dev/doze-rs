//! E2: what an instance prints goes to the log store in Go's layout, and what the engine does goes to the journal in
//! Go's format, where the CLI, the app and the Go readers find them.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod common;

use common::*;
use doze_engine::{Cause, Engine, Step};
use std::time::Duration;

fn texts(e: &Engine, name: &str) -> Vec<String> {
    let (lines, _) = e
        .logs()
        .read(name, doze_logstore::Cursor::default(), 1000)
        .unwrap();
    lines
        .into_iter()
        .map(|l| format!("{} {} {}", l.run, l.stream, l.text))
        .collect()
}

/// D6 closed: a start is a run under `logs/<name>/` (`r000001.s000.log`), its process's lines as `o`, its steps' as
/// `s:<moment>`; the run closes when the process is stopped, and the next start is run 2.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_start_is_a_run_with_its_streams_tagged() {
    let d = Dir::new("logstore");
    let mut spec = listener(&d, 0);
    spec.setup = vec![Step::argv(run(&["say", "2", "0", "setup said"]))];
    spec.on_ready = vec![Step::argv(run(&["say", "1", "0", "ready said"]))];
    let e = Engine::builder("t", d.path())
        .service(svc("web", spec))
        .build()
        .unwrap();
    e.wake("web", Cause::Command).await.unwrap();
    let pid = pid_of(&e, "web");
    assert!(d.path().join("logs/web/r000001.s000.log").is_file());
    assert!(e.logs().runs("web").unwrap()[0].live);
    let got = texts(&e, "web");
    assert_eq!(
        got[..2],
        ["1 s:setup setup said 1", "1 s:setup setup said 2"]
    );
    assert!(
        got.iter().any(|l| l.starts_with("1 o listening on ")),
        "{got:?}"
    );
    assert!(
        got.contains(&"1 s:on_ready ready said 1".to_string()),
        "{got:?}"
    );
    e.stop("web", Cause::Command).await.unwrap();
    assert!(
        got_eventually(&e).await,
        "the run closes once the process is gone"
    );
    assert!(texts(&e, "web").contains(&"1 o stopping".to_string()));
    e.wake("web", Cause::Command).await.unwrap();
    let runs: Vec<u32> = e.logs().runs("web").unwrap().iter().map(|r| r.id).collect();
    assert_eq!(runs, [1, 2]);
    let pid2 = pid_of(&e, "web");
    e.shutdown().await;
    assert!(all_gone(&[pid, pid2]).await);
}

/// A start that fails before its process runs (here its `setup`) closes the run it opened: what the step printed is
/// kept, and the run is no longer the one being written.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_setup_closes_its_run() {
    let d = Dir::new("logsetupfail");
    let mut spec = listener(&d, 0);
    spec.setup = vec![Step::argv(run(&["say", "1", "3", "setup broke"]))];
    let e = Engine::builder("t", d.path())
        .service(svc("web", spec))
        .build()
        .unwrap();
    assert!(e.wake("web", Cause::Command).await.is_err());
    assert_eq!(texts(&e, "web"), ["1 s:setup setup broke 1"]);
    assert!(e.logs().live("web").is_none(), "the run is still open");
    assert!(!e.logs().runs("web").unwrap()[0].live);
    e.shutdown().await;
}

async fn got_eventually(e: &Engine) -> bool {
    eventually(Duration::from_secs(5), || {
        e.logs()
            .runs("web")
            .is_ok_and(|r| r.first().is_some_and(|r| !r.live))
    })
    .await
}

/// The engine's events are journal lines at `run/journal.jsonl`, in order, numbered, with their causes; a second engine
/// on the same folder (a daemon restart) goes on numbering.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn what_the_engine_does_is_journalled() {
    let d = Dir::new("journal");
    let build = || {
        Engine::builder("t", d.path())
            .service(svc("db", listener(&d, 0)))
            .service(svc("api", listener(&d, 0)).needs("db"))
            .build()
            .unwrap()
    };
    let e = build();
    e.wake("api", Cause::Command).await.unwrap();
    let pids = [pid_of(&e, "api"), pid_of(&e, "db")];
    e.shutdown().await;
    assert!(all_gone(&pids).await);
    let path = d.path().join("run/journal.jsonl");
    let read = doze_journal::read(&path).unwrap();
    let got: Vec<(String, String, String, String)> = read
        .iter()
        .map(|ev| {
            (
                ev.kind.clone(),
                ev.instance.clone(),
                ev.cause.kind.clone(),
                ev.cause.by.clone(),
            )
        })
        .collect();
    let want = |k: &str, i: &str, c: &str, b: &str| (k.into(), i.into(), c.into(), b.into());
    assert_eq!(
        got[..2],
        [
            want("wake", "api", "command", ""),
            want("wake", "db", "dependency", "api"),
        ]
    );
    assert!(got.contains(&want("ready", "db", "", "")), "{got:?}");
    assert!(
        got.contains(&want("sleep", "api", "shutdown", "")),
        "{got:?}"
    );
    assert!(read.iter().all(|ev| ev.time.is_some()));
    assert!(
        read.iter()
            .find(|ev| ev.kind == "ready")
            .unwrap()
            .took()
            .is_some()
    );
    let seqs: Vec<u64> = read.iter().map(|ev| ev.seq).collect();
    assert_eq!(seqs, (1..=read.len() as u64).collect::<Vec<_>>());
    // A daemon restart: the next engine goes on from the last number.
    let e2 = build();
    let last = seqs.len() as u64;
    let mut rx = e2.journal().unwrap().subscribe();
    e2.toggle_pin("db").unwrap();
    assert_eq!(rx.recv().await.unwrap().seq, last + 1);
}
