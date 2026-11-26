//! The idle guard: an engine with nothing to do sets no timers. Go's reaper looked every second and its prober every
//! 3 s whatever ran, and the Rust engine inherited both; an idle daemon then woke over a thousand times an hour for
//! nothing. The clock is paused, so an hour passes at once and the count is exact: no timing, nothing to flake.

mod common;

use common::*;
use doze_engine::{Engine, ProcessSpec, Service};
use std::time::Duration;

#[tokio::test(start_paused = true)]
async fn an_idle_engine_sleeps_through_an_hour() {
    let d = Dir::new("idle");
    let e = Engine::builder("t", d.path())
        .idle_timeout(Duration::from_secs(5))
        .service(Service::process("api", ProcessSpec::new(["x"], d.path())))
        .service(Service::process("web", ProcessSpec::new(["x"], d.path())))
        .build()
        .unwrap();
    let bg = e.background();
    tokio::time::sleep(Duration::from_secs(3600)).await;
    // One look each when they start (reaper, prober), then nothing until something changes.
    assert!(
        e.background_wakes() <= 2,
        "the background loops woke {} times in an idle hour",
        e.background_wakes()
    );
    drop(bg);
}
