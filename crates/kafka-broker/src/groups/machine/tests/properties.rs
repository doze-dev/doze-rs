//! Random sequences of requests and time against the machine, checking after every step what must always hold.
//! A failing run replays from its seed: `SIM_SEEDS=1000 cargo test -p kafka-broker --lib groups::machine::tests::properties`.

use super::*;

/// SplitMix64.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) % n
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

const NAMES: [&str; 3] = ["range", "sticky", "cooperative"];

fn seeds() -> std::ops::RangeInclusive<u64> {
    let n = std::env::var("SIM_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    1..=n
}

fn some_member(rng: &mut Rng, g: &Group) -> String {
    let ids = g.machine.member_ids();
    if ids.is_empty() || rng.chance(15) {
        "ghost".into()
    } else {
        ids[rng.below(ids.len() as u64) as usize].to_string()
    }
}

fn some_protocols(rng: &mut Rng) -> Vec<&'static str> {
    let mut names: Vec<&str> = NAMES.into_iter().filter(|_| rng.chance(60)).collect();
    if names.is_empty() {
        names.push(NAMES[rng.below(3) as usize]);
    }
    if rng.chance(50) {
        names.reverse();
    }
    names
}

fn step(rng: &mut Rng, g: &mut Group) {
    match rng.below(10) {
        0..=3 => {
            let existing = rng.chance(40) && !g.machine.member_ids().is_empty();
            let member = existing.then(|| some_member(rng, g));
            let mut req = join_request(member.as_deref(), &some_protocols(rng));
            req.id_first = rng.chance(30);
            req.session_ms = 6_000 + rng.below(30_000) as i32;
            req.rebalance_ms = 1_000 + rng.below(60_000) as i32;
            if rng.chance(25) {
                req.instance = Some(instance(&format!("i{}", rng.below(3))));
            }
            g.join(req);
        }
        4 => {
            let generation = g.machine.generation() + i32::from(rng.chance(15));
            let member = some_member(rng, g);
            let assignments: Vec<(String, String)> = g
                .machine
                .member_ids()
                .iter()
                .map(|m| (m.to_string(), format!("a-{m}")))
                .collect();
            let refs: Vec<(&str, &str)> = assignments
                .iter()
                .map(|(m, a)| (m.as_str(), a.as_str()))
                .collect();
            g.sync(&member, generation, &refs);
        }
        5..=6 => {
            let member = some_member(rng, g);
            g.heartbeat(&member, g.machine.generation());
        }
        7 => {
            let member = some_member(rng, g);
            g.leave(&[member.as_str()]);
        }
        _ => g.advance(rng.below(20_000)),
    }
}

/// What must hold of the group whatever has happened.
fn check(g: &Group, before: i32) {
    let m = &g.machine;
    assert!(
        m.generation() >= before || before > i32::MAX - 10,
        "the generation went down"
    );
    let members = m.member_ids();
    assert_eq!(
        m.state() == State::Empty,
        members.is_empty(),
        "empty if and only if no members: {:?} with {members:?}",
        m.state()
    );
    match m.state() {
        State::Empty => assert!(m.leader().is_none()),
        State::PreparingRebalance => {
            assert!(
                g.timer(&TimerKind::Rebalance).is_some(),
                "a rebalance has a deadline"
            );
        }
        State::CompletingRebalance | State::Stable => {
            let leader = m.leader().expect("a running group has a leader");
            assert!(members.contains(leader), "the leader is a member");
            let chosen = m.protocol().expect("and a protocol");
            for member in m.members.values() {
                assert!(
                    member.protocols.iter().any(|p| p.name == chosen),
                    "{} offers the protocol chosen",
                    member.id
                );
            }
            assert!(
                g.timer(&TimerKind::Rebalance).is_none(),
                "no deadline when not rebalancing"
            );
        }
    }
    for member in &members {
        assert!(
            g.timer(&TimerKind::Session(member.clone())).is_some(),
            "{member} has a session timer"
        );
    }
    for (inst, held) in &m.statics {
        assert_eq!(
            m.members.get(held).and_then(|x| x.instance.as_ref()),
            Some(inst),
            "a static instance names the member that has it"
        );
    }
    for member in m.members.values() {
        if let Some(inst) = &member.instance {
            assert_eq!(m.statics.get(inst), Some(&member.id));
        }
    }
}

#[test]
fn whatever_happens_the_group_keeps_its_invariants_and_empties_when_left_alone() {
    for seed in seeds() {
        let mut rng = Rng(seed);
        let mut g = Group::new(rng.below(3) * 500);
        for n in 0..300 {
            let before = g.machine.generation();
            step(&mut rng, &mut g);
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(&g, before)))
                .unwrap_or_else(|_| panic!("seed {seed}, step {n}: an invariant broke"));
        }
        // Left alone for a long time, every member's session runs out and the group is empty.
        for _ in 0..200 {
            g.advance(60_000);
            if g.machine.state() == State::Empty {
                break;
            }
        }
        assert_eq!(
            g.machine.state(),
            State::Empty,
            "seed {seed}: it did not empty"
        );
        assert!(
            g.unanswered.is_empty(),
            "seed {seed}: requests never answered: {:?}",
            g.unanswered
        );
    }
}
