//! Process trees against real children: a gentle stop, the kill after the grace period, drop, and what happens to
//! an engine when its host dies.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use common::{alive, gone_within, helper, kill_pid};
use doze_os::tree::{Stop, Stopped, Tree};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};

/// Starts a helper as a tree and waits until it says it has installed its signal handlers.
async fn ready_tree(mode: &str) -> Tree {
    let mut cmd = helper(mode);
    cmd.stdout(Stdio::piped()).kill_on_drop(false);
    let mut tree = Tree::spawn(cmd).unwrap();
    let mut line = String::new();
    let out = tree.take_stdout().unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        BufReader::new(out).read_line(&mut line),
    )
    .await
    .expect("no ready line")
    .unwrap();
    assert_eq!(line.trim(), "ready");
    tree
}

#[tokio::test(flavor = "current_thread")]
async fn a_tree_that_listens_stops_gracefully() {
    let mut tree = ready_tree("wait-stop").await;
    let r = tree
        .stop(&Stop::Terminate, Duration::from_secs(10))
        .await
        .unwrap();
    assert!(matches!(r, Stopped::Graceful(st) if st.success()), "{r:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn a_tree_that_ignores_the_stop_is_killed_after_the_grace_period() {
    let mut tree = ready_tree("ignore-stop").await;
    let r = tree
        .stop(&Stop::Terminate, Duration::from_millis(500))
        .await
        .unwrap();
    assert!(matches!(r, Stopped::Forced(_)), "{r:?}");
}

/// Go's database stop: an interrupt that is not heeded is followed by SIGQUIT (Unix) for 3 s, then the kill.
#[tokio::test(flavor = "current_thread")]
async fn an_unheeded_interrupt_is_followed_by_quit_then_the_kill() {
    let mut tree = ready_tree("ignore-stop").await;
    let t = std::time::Instant::now();
    let r = tree
        .stop(&Stop::Interrupt, Duration::from_millis(300))
        .await
        .unwrap();
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    let ended = match r {
        Stopped::Graceful(st) | Stopped::Forced(st) => st,
    };
    assert!(!ended.success(), "{r:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_a_tree_kills_it() {
    let mut cmd = helper("sleep");
    cmd.kill_on_drop(false);
    let tree = Tree::spawn(cmd).unwrap();
    let pid = tree.pid().unwrap();
    drop(tree);
    assert!(
        gone_within(pid, Duration::from_secs(5)).await.is_some(),
        "pid {pid} still running after drop"
    );
}

/// The host (a helper standing in for doze) starts an engine tree and is then killed without warning. Windows (Job
/// object) and Linux (PDEATHSIG) take the engine down; macOS cannot, which the daemon handles at its next start.
#[tokio::test(flavor = "current_thread")]
async fn an_engine_goes_with_its_host_where_the_os_allows() {
    let mut host = helper("host-tree");
    host.stdout(Stdio::piped());
    let mut host = host.spawn().unwrap();
    let mut line = String::new();
    BufReader::new(host.stdout.take().unwrap())
        .read_line(&mut line)
        .await
        .unwrap();
    let engine: u32 = line.trim().parse().unwrap();
    host.start_kill().unwrap();
    host.wait().await.unwrap();
    let gone = gone_within(engine, Duration::from_secs(3)).await;
    if cfg!(target_os = "macos") {
        assert!(
            gone.is_none(),
            "macOS has no parent-death signal; if this changed, update tree/mod.rs"
        );
        kill_pid(engine);
    } else {
        assert!(gone.is_some(), "engine {engine} outlived its host");
    }
}

/// Audit 2, minor 1: a leader that exited and left a child in its tree. Dropping the tree, or waiting for it, takes the
/// child too: the group is signalled before the leader is reaped (its unreaped exit keeps the group id ours).
#[tokio::test(flavor = "current_thread")]
async fn what_an_exited_leader_left_goes_on_drop_and_on_wait() {
    for waited in [false, true] {
        let mut cmd = helper("orphan");
        cmd.stdout(Stdio::piped()).kill_on_drop(false);
        let mut tree = Tree::spawn(cmd).unwrap();
        let mut line = String::new();
        BufReader::new(tree.take_stdout().unwrap())
            .read_line(&mut line)
            .await
            .unwrap();
        let child: u32 = line.trim().parse().unwrap();
        let t = std::time::Instant::now();
        while tree.running().unwrap() {
            assert!(
                t.elapsed() < Duration::from_secs(10),
                "the leader never exited"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            alive(child),
            "the child outlives its leader until the tree ends"
        );
        if waited {
            assert!(tree.wait().await.unwrap().success());
        } else {
            drop(tree);
        }
        assert!(
            gone_within(child, Duration::from_secs(5)).await.is_some(),
            "waited={waited}: child {child} left running"
        );
    }
}

/// Audit 2, minor 6: a tree spawned from a thread that then exits (a blocking thread gone idle) keeps running. On Linux
/// the parent-death signal follows the forking thread, so doze-os forks every tree from one lasting thread.
#[test]
fn a_tree_outlives_the_thread_that_asked_for_it() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .thread_keep_alive(Duration::from_millis(50))
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let tree = tokio::task::spawn_blocking(|| {
            let mut cmd = helper("sleep");
            cmd.kill_on_drop(false);
            Tree::spawn(cmd).unwrap()
        })
        .await
        .unwrap();
        let pid = tree.pid().unwrap();
        // The blocking thread exits after 50 ms idle.
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert!(alive(pid), "the tree died with the thread that spawned it");
        drop(tree);
        assert!(gone_within(pid, Duration::from_secs(5)).await.is_some());
    });
}
