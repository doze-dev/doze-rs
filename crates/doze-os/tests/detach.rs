//! A detached daemon outlives the command that started it and can still stop an engine gently. On Windows that is
//! the point of giving it a hidden console: CTRL_BREAK needs one (detach.rs).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use common::helper;
use std::time::{Duration, Instant};

#[tokio::test(flavor = "current_thread")]
async fn a_detached_daemon_outlives_its_launcher_and_stops_an_engine_gracefully() {
    let out = std::env::temp_dir().join(format!("doze-os-daemon-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let mut launcher = helper("launch-daemon");
    launcher.arg(&out);
    let launched = launcher.output().await.unwrap();
    assert!(
        launched.status.success(),
        "{}",
        String::from_utf8_lossy(&launched.stderr)
    );
    // The launcher has exited; the daemon reports later.
    let t0 = Instant::now();
    let report = loop {
        if let Ok(s) = std::fs::read_to_string(&out) {
            break s;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "the detached daemon never reported"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let _ = std::fs::remove_file(&out);
    assert!(
        report.starts_with("Graceful"),
        "the daemon could not stop its engine gently: {report}"
    );
}
