//! Races the third and fourth audits reproduced at the machine level: a crash while an `on_ready` rerun is in flight
//! (and its step reporting after the cancel), and a stop between a restart timer going off and its wake.

use doze_engine::lifecycle::Now;
use doze_engine::machine::{Cancel, Decision, Effect, Machine, Unit};
use doze_engine::restart::{Policy, Restart};
use doze_engine::{Cause, State};
use std::time::Duration;

fn supervised() -> Unit {
    Unit {
        pids: vec![1],
        backend: String::new(),
        restart: Some(Restart {
            policy: Policy::OnFailure,
            backoff: Duration::from_millis(10),
            max_retries: 3,
        }),
        supervised: true,
        always_on: true,
        idle_after: None,
    }
}

/// A running unit of `w` and its generation.
#[allow(clippy::panic)] // test code: a failure here is the test failing
fn running(m: &mut Machine, now: Now) -> (u64, u64) {
    let (d, _) = m.wake("w", Cause::Command);
    let Decision::Start(b) = d else {
        panic!("{d:?}")
    };
    let (g, ok, _) = m.register("w", b, supervised(), now);
    assert!(ok);
    (b, g)
}

#[allow(clippy::unwrap_used)] // test code
fn timer_of(fx: &[Effect]) -> u64 {
    fx.iter()
        .find_map(|f| match f {
            Effect::Schedule { timer, .. } => Some(*timer),
            _ => None,
        })
        .unwrap()
}

/// Audit 3, M1: the process crashes while an `on_ready` rerun runs. The rerun is cancelled with the crash, the
/// restart's wake waits for it and then starts afresh; nothing is left `crashing` with no restart pending.
#[test]
fn a_crash_during_an_on_ready_rerun_keeps_its_restart() {
    let mut m = Machine::new(["w".to_string()], "doze.hcl", None);
    let now = Now::current();
    let (b, g) = running(&mut m, now);
    let _ = m.on_ready_failed("w", b, "on_ready failed", Duration::ZERO, now);
    let (d, _) = m.wake("w", Cause::Command);
    let Decision::Start(rerun) = d else {
        panic!("{d:?}")
    };
    let fx = m.exited("w", g, false, "exit status 1", now);
    assert!(
        fx.iter()
            .any(|f| matches!(f, Effect::CancelBoot { boot, why: Cancel::Exited, .. } if *boot == rerun)),
        "{fx:?}"
    );
    let fx = m.restart_due("w", timer_of(&fx));
    assert!(fx.iter().any(|f| matches!(f, Effect::RestartNow { .. })));
    assert_eq!(
        m.wake("w", Cause::Restart).0,
        Decision::Busy,
        "waits for the rerun to go"
    );
    let _ = m.boot_cancelled("w", rerun, now);
    assert_eq!(
        m.table().state("w"),
        Some(State::Crashing),
        "the crash's state stays"
    );
    assert!(m.pending("w"));
    // The restart's wake, retried, starts afresh.
    let (d, _) = m.wake("w", Cause::Restart);
    assert!(matches!(d, Decision::Start(_)), "{d:?}");
}

/// Audit 3, minor 1: a stop after a restart timer went off but before its wake ran wins; the wake then starts nothing.
#[test]
fn a_stop_between_a_restart_and_its_wake_wins() {
    let mut m = Machine::new(["w".to_string()], "doze.hcl", None);
    let now = Now::current();
    let (b, g) = running(&mut m, now);
    let _ = m.boot_done("w", b, Duration::ZERO, now);
    let fx = m.exited("w", g, false, "exit status 1", now);
    let fx = m.restart_due("w", timer_of(&fx));
    assert!(fx.iter().any(|f| matches!(f, Effect::RestartNow { .. })));
    let _ = m.stop("w", Cause::Command).unwrap();
    assert_eq!(m.table().state("w"), Some(State::Reaped));
    let (d, _) = m.wake("w", Cause::Restart);
    assert!(matches!(d, Decision::Refuse(_)), "{d:?}");
    assert!(m.quiet());
}

/// Audit 4, N2: the rerun's step fails (its server just died) and reports before the start saw the cancel. It counts as
/// cancelled: the crash's state and its restart stay, and `failed` never shows while a restart is pending.
#[test]
fn a_rerun_failing_after_the_crash_cancelled_it_is_cancelled() {
    let mut m = Machine::new(["w".to_string()], "doze.hcl", None);
    let now = Now::current();
    let (b, g) = running(&mut m, now);
    let _ = m.on_ready_failed("w", b, "on_ready failed", Duration::ZERO, now);
    let (d, _) = m.wake("w", Cause::Command);
    let Decision::Start(rerun) = d else {
        panic!("{d:?}")
    };
    let fx = m.exited("w", g, false, "exit status 1", now);
    let timer = timer_of(&fx);
    let fx = m.on_ready_failed("w", rerun, "on_ready step failed", Duration::ZERO, now);
    assert!(fx.is_empty(), "{fx:?}");
    let i = m.table().get("w", now).unwrap();
    assert_eq!(i.state, State::Crashing);
    assert!(i.last_error.starts_with("exit status 1; restarting"), "{i:?}");
    assert!(m.pending("w"));
    let fx = m.restart_due("w", timer);
    assert!(fx.iter().any(|f| matches!(f, Effect::RestartNow { .. })));
    let (d, _) = m.wake("w", Cause::Restart);
    assert!(matches!(d, Decision::Start(_)), "{d:?}");
}

/// Audit 4, N2: the same for a rerun that reports it is ready after the crash: it is cancelled, not ready.
#[test]
fn a_rerun_done_after_the_crash_cancelled_it_is_cancelled() {
    let mut m = Machine::new(["w".to_string()], "doze.hcl", None);
    let now = Now::current();
    let (b, g) = running(&mut m, now);
    let _ = m.on_ready_failed("w", b, "on_ready failed", Duration::ZERO, now);
    let (d, _) = m.wake("w", Cause::Command);
    let Decision::Start(rerun) = d else {
        panic!("{d:?}")
    };
    let _ = m.exited("w", g, false, "exit status 1", now);
    let (ok, fx) = m.boot_done("w", rerun, Duration::ZERO, now);
    assert!(!ok && fx.is_empty(), "{fx:?}");
    assert_eq!(m.table().state("w"), Some(State::Crashing));
    assert!(m.pending("w"));
}

/// Audit 4, N3: a restart whose timer went off and whose wake has not run yet keeps the stack from being quiet.
#[test]
fn a_restart_waiting_for_its_wake_is_not_quiet() {
    let mut m = Machine::new(["w".to_string()], "doze.hcl", None);
    let now = Now::current();
    let (b, g) = running(&mut m, now);
    let _ = m.boot_done("w", b, Duration::ZERO, now);
    let fx = m.exited("w", g, false, "exit status 1", now);
    assert!(!m.quiet(), "a timer is pending");
    let fx = m.restart_due("w", timer_of(&fx));
    assert!(fx.iter().any(|f| matches!(f, Effect::RestartNow { .. })));
    assert!(!m.quiet(), "the restart's wake is pending");
    assert!(m.pending("w"));
}
