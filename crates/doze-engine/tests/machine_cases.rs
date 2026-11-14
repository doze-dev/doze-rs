//! The machine's decisions in the cases Go names one by one (the proptest in `machine_props.rs` covers interleavings):
//! a pending restart superseded by a start, cancelled by a stop and by the shutdown, and a start stopped before it
//! registered.

use doze_engine::lifecycle::Now;
use doze_engine::machine::{Cancel, Decision, Effect, Machine, Unit};
use doze_engine::restart::{Policy, Restart};
use doze_engine::{Cause, State};
use std::time::Duration;

fn machine() -> Machine {
    Machine::new(["api".to_string()], "doze.hcl", None)
}

fn unit() -> Unit {
    Unit {
        pids: vec![1],
        backend: "127.0.0.1:1".into(),
        restart: Some(Restart {
            policy: Policy::OnFailure,
            backoff: Duration::from_secs(3600),
            max_retries: 5,
        }),
        supervised: true,
        always_on: false,
        idle_after: None,
    }
}

#[allow(clippy::panic)] // test code: a failure here is the test failing
fn started(m: &mut Machine) -> u64 {
    match m.wake("api", Cause::Command).0 {
        Decision::Start(b) => b,
        d => panic!("not a start: {d:?}"),
    }
}

/// Runs `api` and crashes it: the restart timer it scheduled.
#[allow(clippy::panic, clippy::unwrap_used)] // test code
fn crashed(m: &mut Machine) -> u64 {
    let boot = started(m);
    let (generation, ok, _) = m.register("api", boot, unit(), Now::current());
    assert!(ok);
    let _ = m.boot_done("api", boot, Duration::ZERO, Now::current());
    let fx = m.exited(
        "api",
        generation,
        false,
        "exited with status 1",
        Now::current(),
    );
    let timer = fx.iter().find_map(|f| match f {
        Effect::Schedule { timer, .. } => Some(*timer),
        _ => None,
    });
    timer.unwrap_or_else(|| panic!("no restart scheduled: {fx:?}"))
}

#[test]
fn a_start_supersedes_a_pending_restart() {
    let mut m = machine();
    let timer = crashed(&mut m);
    let _ = started(&mut m);
    assert!(
        m.restart_due("api", timer).is_empty(),
        "the old timer is void"
    );
}

#[test]
fn a_stop_cancels_a_pending_restart() {
    let mut m = machine();
    let timer = crashed(&mut m);
    let _ = m.stop("api", Cause::Command).unwrap();
    assert!(m.restart_due("api", timer).is_empty());
    let api = m.table().get("api", Now::current()).unwrap();
    assert_eq!((api.state, api.restart_count), (State::Reaped, 0));
    assert!(m.quiet());
}

#[test]
fn the_shutdown_cancels_a_pending_restart() {
    let mut m = machine();
    let timer = crashed(&mut m);
    let _ = m.shutdown();
    assert!(m.restart_due("api", timer).is_empty());
    assert!(m.quiet());
    assert!(matches!(
        m.wake("api", Cause::Command).0,
        Decision::Refuse(_)
    ));
}

/// A stop while the start is in flight: the processes are stopped as soon as they register, and the instance is
/// asleep, not failed (a deliberate difference from Go, where the start completed and the stop was lost).
#[test]
fn a_start_stopped_before_it_registered_is_reaped() {
    let mut m = machine();
    let boot = started(&mut m);
    let fx = m.stop("api", Cause::Command).unwrap();
    assert_eq!(
        fx,
        vec![Effect::CancelBoot {
            name: "api".into(),
            boot,
            why: Cancel::Stopped,
        }]
    );
    assert!(matches!(m.wake("api", Cause::Command).0, Decision::Busy));
    // The cancel had not landed yet: the processes registered first, and are stopped at once.
    let (generation, ok, fx) = m.register("api", boot, unit(), Now::current());
    assert!(!ok);
    assert_eq!(
        fx,
        vec![Effect::StopUnit {
            name: "api".into(),
            generation
        }]
    );
    let _ = m.unit_gone("api", generation, Now::current());
    let api = m.table().get("api", Now::current()).unwrap();
    assert_eq!(api.state, State::Reaped);
    assert!(m.quiet());
}

/// The usual order: the cancel lands before the start registers; the start drops its work and the instance sleeps.
#[test]
fn a_cancelled_start_sleeps_and_frees_the_next_wake() {
    let mut m = machine();
    let boot = started(&mut m);
    let _ = m.stop("api", Cause::Command).unwrap();
    let _ = m.boot_cancelled("api", boot, Now::current());
    assert_eq!(
        m.table().get("api", Now::current()).unwrap().state,
        State::Reaped
    );
    assert!(m.quiet());
    assert!(matches!(
        m.wake("api", Cause::Command).0,
        Decision::Start(_)
    ));
}
