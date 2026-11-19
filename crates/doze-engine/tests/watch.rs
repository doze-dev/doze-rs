//! E2: `watch` restarts a running service when a file it watches changes (Go's `startWatch`), journalled with cause
//! `watch` naming the path; a stopped service is not watched.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod common;

use common::*;
use doze_engine::events::Kind;
use doze_engine::{Cause, Engine, State};
use std::time::Duration;

fn drain(rx: &mut tokio::sync::broadcast::Receiver<doze_engine::Event>) -> Vec<doze_engine::Event> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        out.push(ev);
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_changed_file_restarts_the_service_and_says_which() {
    let d = Dir::new("watchrestart");
    let src = d.file("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("main.go"), "package main").unwrap();
    let e = Engine::builder("t", d.path())
        .service(svc("api", listener(&d, 0)).watch(&src, &["**/*.go"]))
        .build()
        .unwrap();
    let mut rx = e.events();
    e.wake("api", Cause::Command).await.unwrap();
    let first = pid_of(&e, "api");
    tokio::time::sleep(Duration::from_millis(1200)).await;
    std::fs::write(src.join("notes.txt"), "not watched").unwrap();
    std::fs::write(src.join("main.go"), "package main // edited").unwrap();
    let restarted = eventually(Duration::from_secs(10), || {
        e.get("api")
            .is_some_and(|i| i.state == State::Active || i.state == State::Idle)
            && pid_of(&e, "api") != first
    })
    .await;
    assert!(restarted, "the edit restarted it");
    let second = pid_of(&e, "api");
    let path = src.join("main.go").to_string_lossy().into_owned();
    let got: Vec<(Kind, Cause)> = drain(&mut rx)
        .into_iter()
        .filter(|ev| matches!(ev.kind, Kind::Sleep | Kind::Wake))
        .map(|ev| (ev.kind, ev.cause))
        .collect();
    assert_eq!(
        got,
        [
            (Kind::Wake, Cause::Command),
            (Kind::Sleep, Cause::Watch(path.clone())),
            (Kind::Wake, Cause::Watch(path))
        ]
    );
    // Stopped, it is no longer watched: an edit wakes nothing.
    e.stop("api", Cause::Command).await.unwrap();
    std::fs::write(src.join("main.go"), "package main // again").unwrap();
    tokio::time::sleep(Duration::from_millis(3000)).await;
    assert_eq!(e.get("api").unwrap().state, State::Reaped);
    e.shutdown().await;
    assert!(all_gone(&[first, second]).await);
}
