//! The port trap with a real process: an app that ignores the `$PORT` doze gave it is told where it listens instead.

mod common;

use common::*;
use doze_engine::{Cause, Engine};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_app_listening_elsewhere_is_told_so() {
    // Where the system cannot say what listens (no lsof or ss), there is no hint to check.
    if doze_engine::porttrap::listening(std::process::id())
        .await
        .is_none()
    {
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
    let first = err.lines().next().unwrap_or_default();
    assert!(first.starts_with("web not ready: "), "{err}");
    assert!(first.contains(". web listens on :"), "{err}");
    assert!(
        first.contains("that doze gave it; start it with --port $PORT"),
        "{err}"
    );
    e.shutdown().await;
}
