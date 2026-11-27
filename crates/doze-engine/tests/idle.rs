//! The idle guard: an engine wakes its background loops for deadlines and changes, never on a tick. Go's reaper looked
//! every second and its prober every 3 s whatever ran, and the Rust engine inherited both; an idle daemon then woke over
//! a thousand times an hour for nothing.

mod common;

use common::*;
use doze_engine::{Cause, Engine, ProcessSpec, Service, State};
use std::time::Duration;

/// Nothing running: an hour passes on a paused clock (at once, the count exact) and the loops wake only to start.
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

/// A real instance on the real clock (its process and readiness need one): held, it has no deadline and the loops
/// stay asleep (the old reaper ticked every 250 ms at this idle time); released, it sleeps at its deadline, not before;
/// started again, the change wakes the reaper and it sleeps once more. The bounds are loose on purpose: the count is
/// "a couple" against "one per tick", and the deadlines are checked one way only.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_reaper_wakes_for_deadlines_and_changes_only() {
    let d = Dir::new("idle-real");
    let idle = Duration::from_millis(400);
    let e = Engine::builder("t", d.path())
        .idle_timeout(idle)
        .service(svc("a", listener(&d, 0)))
        .build()
        .unwrap();
    let bg = e.background();
    let asleep = || e.get("a").is_some_and(|i| i.state == State::Reaped);
    e.wake("a", Cause::Command).await.unwrap();
    e.acquire("a");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let before = e.background_wakes();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let held = e.background_wakes() - before;
    assert!(held <= 2, "{held} background wakes while held");
    assert!(
        e.get("a").unwrap().state.running(),
        "held: not put to sleep"
    );
    let released = std::time::Instant::now();
    e.release("a");
    assert!(
        eventually(Duration::from_secs(5), asleep).await,
        "slept after release"
    );
    assert!(released.elapsed() >= idle, "slept before its deadline");
    e.wake("a", Cause::Command).await.unwrap();
    assert!(
        eventually(Duration::from_secs(5), asleep).await,
        "a new start is reaped too"
    );
    drop(bg);
    e.shutdown().await;
}
