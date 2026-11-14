//! The sweep at a daemon's start against real processes: records a Go daemon wrote (a pid, no start time) are swept
//! when the pid still leads its own group and started just before the record; anything else is left alone.

mod common;

use common::*;
use doze_os::tree::Tree;
use std::time::Duration;

/// Audit 2, minor 4: a Go daemon's engines are swept at the cut-over (Go's record: `backend-<name>.pid` with the pid
/// alone); a process that only took a recorded pid (it does not lead its own group) is not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_go_record_is_swept_when_it_still_names_a_tree_leader() {
    if !doze_os::process::sweep_needed() {
        return;
    }
    let d = Dir::new("gosweep");
    let run_dir = d.path().join("run");
    std::fs::create_dir_all(&run_dir).unwrap();
    let mut cmd = tokio::process::Command::new(HELPER);
    cmd.arg("ignore").stdin(std::process::Stdio::null());
    let mut leader = Tree::spawn(cmd).unwrap();
    let pid = leader.pid().unwrap();
    std::fs::write(run_dir.join("backend-go.pid"), format!("{pid}\n")).unwrap();
    // Not a tree: it shares this test's group.
    let mut plain = std::process::Command::new(HELPER)
        .arg("ignore")
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();
    std::fs::write(
        run_dir.join("backend-plain.pid"),
        format!("{}\n", plain.id()),
    )
    .unwrap();
    let killed = doze_engine::pidfile::sweep(&run_dir);
    let plain_stayed = doze_os::process::alive(plain.id());
    let _ = plain.kill();
    let _ = plain.wait();
    assert_eq!(killed, [pid]);
    // The leader is this test's child: reaped here, then gone.
    let ended = tokio::time::timeout(Duration::from_secs(5), leader.wait()).await;
    assert!(matches!(ended, Ok(Ok(st)) if !st.success()), "{ended:?}");
    assert!(all_gone(&[pid]).await);
    assert!(plain_stayed, "a process that is not a tree leader stays");
    assert!(
        !run_dir.join("backend-plain.pid").exists(),
        "its record goes"
    );
}

/// Audit 2, minor 8: the sweep refuses while the engine runs anything (its own processes match their records).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_sweep_refuses_once_something_runs() {
    let d = Dir::new("sweepguard");
    let e = doze_engine::Engine::builder("t", d.path())
        .service(svc("w", listener(&d, 0)))
        .build()
        .unwrap();
    assert_eq!(
        e.reconcile().unwrap(),
        Vec::<u32>::new(),
        "quiet: it sweeps"
    );
    e.wake("w", doze_engine::Cause::Command).await.unwrap();
    let pid = pid_of(&e, "w");
    assert!(e.reconcile().is_err());
    assert!(
        doze_os::process::alive(pid),
        "its own process is left alone"
    );
    e.shutdown().await;
    assert!(all_gone(&[pid]).await);
}

/// Audit 4, N4: Go writes its record once the start is ready, which can be well after the spawn (the longest readiness
/// budget, with hooks). A record written a minute after its leader started is swept; one written 20 minutes after is
/// not (the pid was more likely reused).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_go_record_is_swept_up_to_15_minutes_after_the_spawn() {
    if !doze_os::process::sweep_needed() {
        return;
    }
    let d = Dir::new("gosweepwin");
    let run_dir = d.path().join("run");
    std::fs::create_dir_all(&run_dir).unwrap();
    let record = |pid: u32, ahead: Duration| {
        let path = run_dir.join("backend-go.pid");
        std::fs::write(&path, format!("{pid}\n")).unwrap();
        let f = std::fs::File::options().write(true).open(&path).unwrap();
        f.set_modified(std::time::SystemTime::now() + ahead)
            .unwrap();
    };
    let spawn = || {
        let mut cmd = tokio::process::Command::new(HELPER);
        cmd.arg("ignore").stdin(std::process::Stdio::null());
        Tree::spawn(cmd).unwrap()
    };
    let late = spawn();
    let late_pid = late.pid().unwrap();
    record(late_pid, Duration::from_secs(20 * 60));
    let killed = doze_engine::pidfile::sweep(&run_dir);
    let stayed = doze_os::process::alive(late_pid);
    // Dropping the tree kills it (its own pid, exactly).
    drop(late);
    assert!(killed.is_empty(), "{killed:?}");
    assert!(stayed, "a record 20 minutes after the spawn is not swept");
    let mut soon = spawn();
    let pid = soon.pid().unwrap();
    record(pid, Duration::from_secs(60));
    let killed = doze_engine::pidfile::sweep(&run_dir);
    assert_eq!(killed, [pid], "a record a minute after the spawn is swept");
    let ended = tokio::time::timeout(Duration::from_secs(5), soon.wait()).await;
    assert!(matches!(ended, Ok(Ok(st)) if !st.success()), "{ended:?}");
}
