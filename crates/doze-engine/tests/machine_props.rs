//! The engine's decisions under arbitrary interleavings (PLAN.md 2.2). The test plays the shell: it remembers what the
//! machine asked for (starts, units, stops, timers) and feeds back any order of outcomes, and after every input checks:
//!
//! 1. one start at a time per instance;
//! 2. nothing starts, restarts or registers after the shutdown, and the shutdown ends quiet;
//! 3. a restart fires only from a timer no stop, newer start, newer timer or shutdown cancelled (the shell keeps the
//!    cancelled timers and fires them too);
//! 4. holds add up: an instance's count is exactly its clients plus the instances holding it;
//! 5. the reaper never stops a pinned instance;
//! 6. a running instance has a unit; a unit is stopped at most once;
//! 7. a crashing instance is within its restart budget;
//! 8. a crashing instance has its restart pending (a timer, its wake, or a start in flight).

use doze_engine::lifecycle::Now;
use doze_engine::machine::{Decision, Effect, Machine, Unit};
use doze_engine::restart::{Policy, Restart};
use doze_engine::{Cause, State};
use proptest::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

const NAMES: [&str; 3] = ["web", "api", "db"];

#[derive(Debug, Clone)]
enum In {
    Wake(usize),
    Hold(usize),
    Fail(usize),
    Register(usize, bool),
    ReadyFail(usize),
    Done(usize),
    Exit(usize, bool),
    Stop(usize),
    Gone(usize),
    Fire(usize),
    Cancelled(usize),
    Reap(u16),
    Client(usize, bool),
    Pin(usize),
    Shutdown,
}

fn input() -> impl Strategy<Value = In> {
    let i = || 0usize..6;
    prop_oneof![
        3 => (0usize..3).prop_map(In::Wake),
        2 => i().prop_map(In::Hold),
        1 => i().prop_map(In::Fail),
        3 => (i(), any::<bool>()).prop_map(|(a, b)| In::Register(a, b)),
        1 => i().prop_map(In::ReadyFail),
        2 => i().prop_map(In::Done),
        2 => (i(), any::<bool>()).prop_map(|(a, b)| In::Exit(a, b)),
        2 => (0usize..3).prop_map(In::Stop),
        2 => i().prop_map(In::Gone),
        2 => i().prop_map(In::Fire),
        2 => i().prop_map(In::Cancelled),
        1 => (0u16..3000).prop_map(In::Reap),
        2 => ((0usize..3), any::<bool>()).prop_map(|(a, b)| In::Client(a, b)),
        1 => (0usize..3).prop_map(In::Pin),
        1 => Just(In::Shutdown),
    ]
}

#[derive(Default)]
struct Shell {
    boots: BTreeMap<u64, String>,
    units: BTreeMap<u64, String>,
    stops: BTreeMap<u64, String>,
    stopped: BTreeSet<u64>,
    registered: BTreeSet<u64>,
    timers: BTreeMap<u64, String>,
    /// Timers a stop, a newer start, a newer timer or the shutdown cancelled: still fired, and must restart nothing.
    dead: BTreeMap<u64, String>,
    cancels: BTreeMap<u64, String>,
    clients: BTreeMap<String, u32>,
    shut: bool,
}

fn pick<V: Clone>(m: &BTreeMap<u64, V>, i: usize) -> Option<(u64, V)> {
    let n = m.len();
    (n > 0)
        .then(|| m.iter().nth(i % n).map(|(k, v)| (*k, v.clone())))
        .flatten()
}

/// The timers of `name` (all of them with `None`) are cancelled: the shell still fires them later.
fn cancel_timers(s: &mut Shell, name: Option<&str>) {
    let gone: Vec<u64> = s
        .timers
        .iter()
        .filter(|(_, n)| name.is_none_or(|x| x == n.as_str()))
        .map(|(t, _)| *t)
        .collect();
    for t in gone {
        if let Some(n) = s.timers.remove(&t) {
            s.dead.insert(t, n);
        }
    }
}

fn absorb(s: &mut Shell, m: &Machine, fx: Vec<Effect>) {
    for f in fx {
        match f {
            Effect::StartBoot {
                name, boot, rerun, ..
            } => {
                if rerun {
                    // Only `on_ready` again: it registers no processes.
                    s.registered.insert(boot);
                }
                assert!(!s.shut, "a start after the shutdown");
                assert!(
                    !s.boots.values().any(|n| *n == name),
                    "two starts of {name}"
                );
                cancel_timers(s, Some(&name));
                s.boots.insert(boot, name);
            }
            Effect::StopUnit { name, generation } => {
                assert!(
                    s.stopped.insert(generation),
                    "unit {generation} stopped twice"
                );
                s.units.remove(&generation);
                s.stops.insert(generation, name);
            }
            Effect::CancelBoot { name, boot } => {
                s.cancels.insert(boot, name);
            }
            Effect::Schedule { name, timer, .. } => {
                assert!(!s.shut, "a restart scheduled after the shutdown");
                cancel_timers(s, Some(&name));
                s.timers.insert(timer, name);
            }
            Effect::RestartNow { name } => assert!(
                !s.shut && !m.shutting_down(),
                "{name} restarted after the shutdown"
            ),
            _ => {}
        }
    }
}

fn unit(policy: bool) -> Unit {
    let restart = policy.then_some(Restart {
        policy: Policy::OnFailure,
        backoff: Duration::from_millis(10),
        max_retries: 2,
    });
    Unit {
        pids: vec![1],
        backend: "x".into(),
        restart,
        supervised: true,
        always_on: false,
        idle_after: None,
    }
}

fn step(m: &mut Machine, s: &mut Shell, now: &mut Now, i: &In) {
    let d = Duration::ZERO;
    match i {
        In::Wake(n) => {
            let name = NAMES.get(*n).copied().unwrap_or("db");
            let (dec, fx) = m.wake(name, Cause::Command);
            if let Decision::Start(_) = dec {
                assert!(fx.iter().any(|f| matches!(f, Effect::StartBoot { .. })));
            }
            absorb(s, m, fx);
        }
        In::Hold(k) => {
            if let Some((b, name)) = pick(&s.boots, *k) {
                let need = if name == "web" { "api" } else { "db" };
                if need != name {
                    m.need_held(&name, b, need);
                }
            }
        }
        In::Fail(k) => {
            // A start fails this way only before its processes registered (after, it is `ReadyFail` or `Done`).
            let open: BTreeMap<u64, String> = s
                .boots
                .iter()
                .filter(|(b, _)| !s.registered.contains(b))
                .map(|(b, n)| (*b, n.clone()))
                .collect();
            if let Some((b, name)) = pick(&open, *k) {
                s.boots.remove(&b);
                let fx = m.boot_failed(&name, b, "boom", d, *now);
                absorb(s, m, fx);
            }
        }
        In::Register(k, policy) => {
            let open: BTreeMap<u64, String> = s
                .boots
                .iter()
                .filter(|(b, _)| !s.registered.contains(b))
                .map(|(b, n)| (*b, n.clone()))
                .collect();
            if let Some((b, name)) = pick(&open, *k) {
                s.registered.insert(b);
                let (g, ok, fx) = m.register(&name, b, unit(*policy), *now);
                if s.shut {
                    assert!(!ok, "registered after the shutdown");
                }
                if ok {
                    s.units.insert(g, name);
                } else {
                    s.boots.remove(&b);
                }
                absorb(s, m, fx);
            }
        }
        In::ReadyFail(k) => {
            if let Some((b, name)) = pick(&s.boots, *k) {
                s.boots.remove(&b);
                let fx = m.on_ready_failed(&name, b, "x", d);
                absorb(s, m, fx);
            }
        }
        In::Done(k) => {
            if let Some((b, name)) = pick(&s.boots, *k) {
                s.boots.remove(&b);
                let (_, fx) = m.boot_done(&name, b, d, *now);
                absorb(s, m, fx);
            }
        }
        _ => step2(m, s, now, i),
    }
}

fn step2(m: &mut Machine, s: &mut Shell, now: &mut Now, i: &In) {
    match i {
        In::Exit(k, ok) => {
            if let Some((g, name)) = pick(&s.units, *k) {
                s.units.remove(&g);
                let fx = m.exited(&name, g, *ok, "exited", *now);
                absorb(s, m, fx);
            }
        }
        In::Stop(n) => {
            let name = NAMES.get(*n).copied().unwrap_or("db");
            cancel_timers(s, Some(name));
            let fx = m.stop(name, Cause::Command).unwrap_or_default();
            absorb(s, m, fx);
        }
        In::Gone(k) => {
            if let Some((g, name)) = pick(&s.stops, *k) {
                s.stops.remove(&g);
                let fx = m.unit_gone(&name, g, *now);
                absorb(s, m, fx);
            }
        }
        In::Cancelled(k) => {
            // A cancelled start drops what it was doing and says so.
            if let Some((b, name)) = pick(&s.cancels, *k) {
                s.cancels.remove(&b);
                if s.boots.remove(&b).is_some() {
                    let fx = m.boot_cancelled(&name, b, *now);
                    absorb(s, m, fx);
                }
            }
        }
        In::Fire(k) => fire_any(m, s, *k),
        In::Reap(ms) => {
            *now = now.after(Duration::from_millis(u64::from(*ms)));
            let fx = m.reap(*now);
            for f in &fx {
                if let Effect::StopUnit { name, .. } = f {
                    assert!(
                        !m.table().get(name, *now).is_some_and(|i| i.keep_awake),
                        "reaped a pinned {name}"
                    );
                }
            }
            absorb(s, m, fx);
        }
        In::Client(n, up) => {
            let name = NAMES.get(*n).copied().unwrap_or("db").to_string();
            let c = s.clients.entry(name.clone()).or_default();
            if *up {
                *c += 1;
                m.acquire(&name);
            } else if *c > 0 {
                *c -= 1;
                m.release(&name, *now);
            }
        }
        In::Pin(n) => drop(m.toggle_pin(NAMES.get(*n).copied().unwrap_or("db"))),
        In::Shutdown => {
            s.shut = true;
            cancel_timers(s, None);
            let fx = m.shutdown();
            absorb(s, m, fx);
        }
        _ => {}
    }
}

/// Any timer goes off, a live or a cancelled one.
fn fire_any(m: &mut Machine, s: &mut Shell, k: usize) {
    let all: BTreeMap<u64, String> = s
        .timers
        .iter()
        .chain(s.dead.iter())
        .map(|(t, n)| (*t, n.clone()))
        .collect();
    if let Some((t, name)) = pick(&all, k) {
        fire(m, s, t, &name);
    }
}

/// Timer `t` of `name` goes off; one that was cancelled restarts nothing (invariant 3).
fn fire(m: &mut Machine, s: &mut Shell, t: u64, name: &str) {
    let cancelled = s.dead.remove(&t).is_some();
    s.timers.remove(&t);
    let fx = m.restart_due(name, t);
    if cancelled {
        assert!(
            !fx.iter().any(|f| matches!(f, Effect::RestartNow { .. })),
            "{name} restarted from timer {t}, which was cancelled"
        );
    }
    absorb(s, m, fx);
}

fn check(m: &Machine, s: &Shell, now: Now) {
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
                m.pending(&inst.name),
                "{} is crashing with nothing pending",
                inst.name
            );
        }
    }
}

/// Plays every outcome the shell still owes, as a shutdown does.
fn drain(m: &mut Machine, s: &mut Shell, now: Now) {
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
    assert!(m.quiet(), "not quiet after the shutdown");
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 3000, ..ProptestConfig::default() })]
    #[test]
    fn decisions_hold_their_invariants(inputs in proptest::collection::vec(input(), 1..120)) {
        let mut m = Machine::new(NAMES.iter().map(|n| (*n).to_string()), "doze.hcl", Some(Duration::from_secs(1)));
        let (mut s, mut now) = (Shell::default(), Now::current());
        for i in &inputs {
            step(&mut m, &mut s, &mut now, i);
            check(&m, &s, now);
        }
        drain(&mut m, &mut s, now);
    }
}
