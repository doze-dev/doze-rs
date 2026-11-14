//! Two races the third audit reproduced at the machine level: a crash while an `on_ready` rerun is in flight, and a
//! stop between a restart timer going off and its wake.

use doze_engine::lifecycle::Now;
use doze_engine::machine::{Decision, Effect, Machine, Unit};
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
    let _ = m.on_ready_failed("w", b, "on_ready failed", Duration::ZERO);
    let (d, _) = m.wake("w", Cause::Command);
    let Decision::Start(rerun) = d else {
        panic!("{d:?}")
    };
    let fx = m.exited("w", g, false, "exit status 1", now);
    assert!(
        fx.iter()
            .any(|f| matches!(f, Effect::CancelBoot { boot, .. } if *boot == rerun)),
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
