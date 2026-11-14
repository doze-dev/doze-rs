//! The restart wakes in `machine_props` (invariant 3), and a case CI found, replayed whatever the seed.

use super::*;

/// The engine's restart wake for a `RestartNow`: `Busy` is asked again later, anything else is its answer.
pub(super) fn restart_wake(m: &mut Machine, s: &mut Shell, k: usize) {
    let n = s.restarts.len();
    if n == 0 {
        return;
    }
    let Some((name, live)) = s.restarts.get(k % n).cloned() else {
        return;
    };
    let (dec, fx) = m.wake(&name, Cause::Restart);
    // Restart wakes carry no token: a stale one that lands while a newer restart of the same instance is owed serves
    // that one (one start), and the newer wake then finds nothing owed (Windows CI, E1 run 43601464531).
    let newer = s
        .restarts
        .iter()
        .position(|(n, l)| *n == name && *l)
        .filter(|_| !live && matches!(dec, Decision::Start(_)));
    // Invariant 3, for the wake: a cancelled restart starts nothing of its own.
    assert!(
        live || newer.is_some() || !matches!(dec, Decision::Start(_)),
        "{name}: a cancelled restart started it"
    );
    if let Some(r) = newer.and_then(|j| s.restarts.get_mut(j)) {
        r.1 = false;
    }
    if dec != Decision::Busy {
        s.restarts.remove(k % n);
    }
    absorb(s, m, fx);
}

/// The case proptest found on Windows CI for E1 (run 43601464531), replayed whatever the seed.
#[test]
fn the_windows_ci_case_holds() {
    use In::*;
    let inputs = [
        Wake(0),
        Register(0, true),
        Exit(0, false),
        Fire(0),
        Cancelled(0),
        Wake(1),
        Wake(0),
        Done(2),
        Register(0, true),
        ReadyFail(0),
        Exit(0, false),
        Fire(0),
        RestartWake(2),
    ];
    let mut m = Machine::new(
        NAMES.iter().map(|n| (*n).to_string()),
        "doze.hcl",
        Some(Duration::from_secs(1)),
    );
    let (mut s, mut now) = (Shell::default(), Now::current());
    for i in &inputs {
        step(&mut m, &mut s, &mut now, i);
        check(&m, &s, now);
    }
    drain(&mut m, &mut s, now);
}
