//! A task stopped while it decides whether it has to run: the start ends at once, and the read of its inputs gives up
//! too (E2 audit 3, N1 and N10).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
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

/// A stop while a task is still deciding whether it has to run (its inputs are being read: a 1 TiB sparse file, which a
/// debug build hashes for minutes) ends the start at once: nothing runs, nothing is journalled or recorded, it is
/// asleep, and the waiter hears why (E2 audit 3, N1 and N10). The read itself gives up too, so nothing is left for
/// the runtime's shutdown to wait on. The test once read a FIFO no one wrote, and hung CI for 3 h when the read
/// outlived the test; its own runtime now has a hard limit on the body and on the shutdown, so a regression fails in
/// seconds.
#[test]
fn a_task_stopped_while_it_decides_ends_at_once_and_says_why() {
    // NTFS does not make a file sparse by extending it: 1 TiB would be real. Unix filesystems keep it a hole.
    if cfg!(windows) {
        return;
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let body = rt.block_on(async {
        tokio::time::timeout(Duration::from_secs(30), stopped_while_deciding()).await
    });
    let t = std::time::Instant::now();
    rt.shutdown_timeout(Duration::from_secs(10));
    body.expect("the test body hung");
    assert!(
        t.elapsed() < Duration::from_secs(10),
        "the abandoned read kept the runtime from shutting down"
    );
}

async fn stopped_while_deciding() {
    let d = Dir::new("taskdecide");
    let ran = d.file("ran");
    // Sparse: no disk, and reads of zeros at memory speed, still minutes of hashing.
    std::fs::File::create(d.file("big.bin"))
        .unwrap()
        .set_len(1 << 40)
        .unwrap();
    let mut ts = task(&d, &["append", &ran.to_string_lossy()]);
    ts.inputs = vec!["big.bin".into()];
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
