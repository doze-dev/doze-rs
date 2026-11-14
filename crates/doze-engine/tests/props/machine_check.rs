//! The checks after every input and the drain at the end of `machine_props` (invariants 1 and 4 to 10).

use super::*;

pub(super) fn check(m: &Machine, s: &Shell, now: Now) {
    for name in NAMES {
        let live = s
            .units
            .values()
            .chain(s.stops.values())
            .filter(|n| *n == name)
            .count();
        assert!(live <= 1, "{name} has {live} copies running at once");
    }
    for inst in m.table().snapshot(now) {
        let clients = s.clients.get(&inst.name).copied().unwrap_or(0);
        if inst.state.running() {
            let has = s
                .units
                .values()
                .chain(s.stops.values())
                .any(|n| *n == inst.name);
            assert!(has, "{} is {:?} with no unit", inst.name, inst.state);
            assert_eq!(
                inst.conns,
                clients + m.held_by_instances(&inst.name),
                "holds of {}",
                inst.name
            );
        }
        if inst.state == State::Crashing {
            assert!(inst.restart_count <= inst.restart_max, "{inst:?}");
            // Invariant 8: `crashing` promises a restart: a timer, its wake, or a start in flight (audit 3, M1).
            assert!(
                m.pending(&inst.name) && owed(s, &inst.name),
                "{} is crashing with nothing pending",
                inst.name
            );
        }
        if inst.state == State::Failed {
            // Invariant 9: at most a rerun, or a start whose cancel is on its way, is in flight.
            let rerun = s.boots.iter().any(|(b, n)| {
                *n == inst.name && (s.reruns.contains(b) || s.cancels.contains_key(b))
            });
            assert!(
                !m.pending(&inst.name) || rerun,
                "{} is failed with a restart pending",
                inst.name
            );
            assert!(
                !s.timers.values().any(|n| *n == inst.name),
                "{} is failed with a live timer",
                inst.name
            );
        }
    }
}

/// Plays every outcome the shell still owes, as a shutdown does.
pub(super) fn drain(m: &mut Machine, s: &mut Shell, now: Now) {
    let fx = m.shutdown();
    absorb(s, m, fx);
    for _ in 0..50 {
        let boots: Vec<(u64, String)> = s.boots.iter().map(|(b, n)| (*b, n.clone())).collect();
        for (b, name) in boots {
            s.boots.remove(&b);
            // A start that has not registered yet does so now (and is stopped: the stack is shutting down); one that
            // has finishes.
            let fx = if s.registered.insert(b) {
                m.register(&name, b, unit(true), now).2
            } else {
                m.boot_done(&name, b, Duration::ZERO, now).1
            };
            absorb(s, m, fx);
        }
        let stops: Vec<(u64, String)> = s.stops.iter().map(|(g, n)| (*g, n.clone())).collect();
        for (g, name) in stops {
            s.stops.remove(&g);
            let fx = m.unit_gone(&name, g, now);
            absorb(s, m, fx);
        }
        if s.boots.is_empty() && s.stops.is_empty() {
            break;
        }
    }
    let left: Vec<(u64, String)> = s
        .timers
        .iter()
        .chain(s.dead.iter())
        .map(|(t, n)| (*t, n.clone()))
        .collect();
    for (t, name) in left {
        fire(m, s, t, &name);
    }
    for (name, _) in std::mem::take(&mut s.restarts) {
        let (dec, fx) = m.wake(&name, Cause::Restart);
        assert!(matches!(dec, Decision::Refuse(_)), "{name}: {dec:?}");
        absorb(s, m, fx);
    }
    assert!(m.quiet(), "not quiet after the shutdown");
}

