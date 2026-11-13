//! Privilege: a process that goes through `relaunch_unprivileged` ends up unprivileged. The Windows CI runners run
//! elevated, so there this test exercises the restricted-token re-launch for real.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use common::helper;

async fn run(mode: &str) -> String {
    let out = helper(mode).output().await.unwrap();
    assert!(
        out.status.success(),
        "{mode}: {} {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[tokio::test(flavor = "current_thread")]
async fn after_relaunch_nothing_is_privileged() {
    // Recorded, not asserted: a developer's own session may or may not be elevated.
    eprintln!("this session: {}", run("privilege").await);
    assert_eq!(run("relaunch-privilege").await, "privileged=false");
}
