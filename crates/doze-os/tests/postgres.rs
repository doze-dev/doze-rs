//! A real postgres started directly as a tree, after `relaunch_unprivileged`: on the elevated Windows runners this
//! is the proof that engines run with doze's restricted token (postgres refuses an elevated one, W0). Runs only where
//! `PGBIN` names a postgres bin directory (CI sets it on Windows x64 and Linux).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

#[tokio::test(flavor = "current_thread")]
async fn postgres_runs_as_a_tree_after_dropping_privileges() {
    let Some(bindir) = std::env::var_os("PGBIN") else {
        eprintln!("PGBIN not set: skipped");
        return;
    };
    let work = std::env::temp_dir().join(format!("doze-os-pg-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    let mut cmd = common::helper("pg");
    cmd.arg(&bindir).arg(&work);
    let out = tokio::time::timeout(std::time::Duration::from_secs(180), cmd.output())
        .await
        .expect("pg helper hung")
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    eprintln!("{stdout}{}", String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(&work);
    assert!(out.status.success(), "helper failed: {}", out.status);
    assert!(stdout.contains("pg=ready"), "postgres never became ready");
    assert!(
        stdout.contains("pg=stopped Graceful"),
        "postgres did not stop gracefully"
    );
}
