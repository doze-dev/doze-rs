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

/// An instance none of whose listeners could be opened is failed with the reason, every start of it is refused with
/// that reason (an `up`, or a wake by what needs it), and a stop does not make it look asleep.
#[test]
#[allow(clippy::panic)] // test code
fn an_unreachable_instance_is_failed_and_refuses_every_start() {
    let mut m = machine();
    let why = "api cannot listen on 127.0.0.1:4567: Address already in use";
    let fx = m.set_unreachable("api", why).unwrap_or_default();
    assert!(fx.iter().any(|f| matches!(f, Effect::Journal(_))), "{fx:?}");
    let shown = |m: &Machine| m.table().get("api", Now::current()).unwrap_or_default();
    assert_eq!(shown(&m).state, State::Failed);
    assert_eq!(shown(&m).last_error, why);
    match m.wake("api", Cause::Command).0 {
        Decision::Refuse(e) => assert_eq!(e.to_string(), why),
        d => panic!("not refused: {d:?}"),
    }
    let _ = m.stop("api", Cause::Command);
    assert_eq!(shown(&m).state, State::Failed);
    assert_eq!(shown(&m).last_error, why);
    assert!(m.set_unreachable("nope", why).is_err(), "not declared");
}

/// Starts `name` (holding `needs`) and runs it: its unit's generation.
#[allow(clippy::panic)] // test code
fn running(m: &mut Machine, name: &str, needs: &[&str]) -> u64 {
    let Decision::Start(boot) = m.wake(name, Cause::Command).0 else {
        panic!("{name} did not start");
    };
    for n in needs {
        m.need_held(name, boot, n);
    }
    let (generation, ok, _) = m.register(name, boot, unit(), Now::current());
    assert!(ok, "{name} did not register");
    let _ = m.boot_done(name, boot, Duration::ZERO, Now::current());
    generation
}

/// The units a set of effects stops, by name.
fn stops(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .filter_map(|f| match f {
            Effect::StopUnit { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

/// The shutdown stops a consumer before its broker: the 2026-11-28 larder `down` stopped kafka with its consumers, and
/// one of them waited on its goodbye to kafka for its whole grace, then was killed. What it needs goes once it is gone.
#[test]
fn the_shutdown_stops_dependants_before_what_they_need() {
    let mut m = Machine::new(["web", "api", "db"].map(String::from), "doze.hcl", None);
    let db = running(&mut m, "db", &[]);
    let api = running(&mut m, "api", &["db"]);
    let web = running(&mut m, "web", &["api", "db"]);
    assert_eq!(
        stops(&m.shutdown()),
        ["web"],
        "only what nothing needs first"
    );
    // Asked again meanwhile (the engine's wait for quiet does): still only what is free.
    assert!(stops(&m.stop_all(Cause::Shutdown)).is_empty());
    assert_eq!(stops(&m.unit_gone("web", web, Now::current())), ["api"]);
    assert_eq!(stops(&m.unit_gone("api", api, Now::current())), ["db"]);
    assert!(stops(&m.unit_gone("db", db, Now::current())).is_empty());
    assert!(m.quiet());
}

/// Needs that go round in a circle cannot wait for each other: with nothing free and nothing on its way out, all stop.
#[test]
fn needs_in_a_circle_still_stop() {
    let mut m = Machine::new(["a", "b"].map(String::from), "doze.hcl", None);
    let a = running(&mut m, "a", &["b"]);
    let b = running(&mut m, "b", &["a"]);
    let mut first = stops(&m.shutdown());
    first.sort();
    assert_eq!(first, ["a", "b"]);
    let _ = m.unit_gone("a", a, Now::current());
    let _ = m.unit_gone("b", b, Now::current());
    assert!(m.quiet());
}

/// A stop of everything that is not the shutdown (`Engine::stop_all`) orders the same way; its caller asks again as
/// each one goes.
#[test]
fn stop_all_leaves_what_is_still_needed_for_a_later_pass() {
    let mut m = Machine::new(["api", "db"].map(String::from), "doze.hcl", None);
    let db = running(&mut m, "db", &[]);
    let api = running(&mut m, "api", &["db"]);
    assert_eq!(stops(&m.stop_all(Cause::Command)), ["api"]);
    assert!(
        stops(&m.unit_gone("api", api, Now::current())).is_empty(),
        "no shutdown: the caller asks again"
    );
    assert_eq!(stops(&m.stop_all(Cause::Command)), ["db"]);
    let _ = m.unit_gone("db", db, Now::current());
    assert!(m.quiet());
}
