//! Liveness of a consumer group under changing DESCRIBE rights (SEC-230 to SEC-233): clients that behave (they heartbeat
//! every 500 ms, rejoin when told to, give up what they are told to) of the consumer protocol and the classic one, while
//! the ACLs flap; then the ACLs stop and the group must settle: its epoch stops moving, no classic client is told to
//! rejoin, every partition a member may have is owned once, and nobody holds a topic it may not DESCRIBE. And the
//! shape of one right taken away: the classic member hears of it at its next heartbeat, inside the assignment interval
//! too. Written by an independent audit; the generator (`liveness_sim`, `liveness_clients`) is seeded and reproducible.

use super::liveness_sim::{Sim, T};
use super::sim::Rng;
use super::*;
use crate::groups::machine::LeaveTarget;

/// Seeds of the flapping scenario, each run with the targets computed at once and with an interval (1 s) between
/// computations. The audit ran the same 200 (about 6 s in a debug build); 400 pass too.
const SEEDS: u64 = 200;

/// Builds the group: three consumer-protocol members, then three classic ones (the first is the one that makes the
/// group a consumer group), each of a random principal and with random topics.
fn populate(s: &mut Sim, seed: u64) {
    let mut rng = Rng(seed ^ 0xabcd_ef12_3456_789b | 1);
    let regexes = [None, None, Some("t.*"), Some("t[0-1]"), Some("t[2-3]")];
    let (all_carol, all_bob) = (seed.is_multiple_of(5), seed.is_multiple_of(7));
    for k in 0..6 {
        let who = if all_carol {
            2
        } else if all_bob {
            1
        } else {
            rng.below(3)
        };
        let mut subs: Vec<&str> = Vec::new();
        for _ in 0..=rng.below(3) {
            let t = T[rng.below(4)];
            if !subs.contains(&t) {
                subs.push(t);
            }
        }
        if k < 3 {
            let regex = regexes[rng.below(5)];
            s.add_cons(&format!("c{k}"), who, &subs, regex);
        } else {
            let coop = rng.pct(50);
            s.add_classic(&format!("k{}", k - 3), who, &subs, coop);
        }
    }
}

/// Runs one scenario: warm-up, 120 ticks with flips, 120 without. `Err`: what went wrong.
fn scenario(seed: u64, assign_ms: u64) -> Result<(), String> {
    let mut s = Sim::new(seed, assign_ms);
    populate(&mut s, seed);
    for _ in 0..6 {
        s.tick();
    }
    let mut spacing = 1 + s.rng.below(12);
    for t in 0..120 {
        if t % spacing == 0 {
            s.flip();
            spacing = 1 + s.rng.below(12);
        }
        s.tick();
        s.check(&format!("flap {t}"));
    }
    let mut views = Vec::new();
    for t in 0..120 {
        s.tick();
        views.push(s.check(&format!("settle {t}")));
    }
    let last = views.last().unwrap().clone();
    let at = |what: &str| format!("seed {seed} assign {assign_ms}: {what}");
    if let Some(i) = views[90..]
        .iter()
        .position(|v| format!("{v:?}") != format!("{last:?}"))
    {
        return Err(at(&format!(
            "not stable in the last 30 ticks (diff at {i})"
        )));
    }
    let hidden = s.hidden_held(&last);
    if !hidden.is_empty() {
        return Err(at(&format!("hidden held at rest: {hidden:?}")));
    }
    let unowned = s.unowned(&last);
    if !unowned.is_empty() {
        return Err(at(&format!("unowned at rest: {unowned:?}")));
    }
    if !s.overlaps.is_empty() {
        return Err(at(&format!("overlap {:?}", s.overlaps)));
    }
    Ok(())
}

#[test]
fn a_group_settles_after_the_acls_stop_flapping() {
    let bad: Vec<String> = (1..=SEEDS)
        .flat_map(|seed| [0, 1_000].map(|assign| (seed, assign)))
        .filter_map(|(seed, assign)| scenario(seed, assign).err())
        .collect();
    assert!(bad.is_empty(), "{} failures: {bad:#?}", bad.len());
}

/// Two classic members left in a consumer group by an admin's LeaveGroup of its only consumer-protocol member (and a
/// third that joined 2 s before the right is taken, when `join_a_member`: the targets were computed just now): bob's
/// `k0` loses `t1`. The tick (of 500 ms) at which the client is first told to rejoin, and at which it stops holding `t1`.
fn right_taken(assign_ms: u64, join_a_member: bool) -> (Option<usize>, Option<usize>) {
    let mut s = Sim::new(7, assign_ms);
    s.add_cons("c0", 0, &["t0"], None);
    s.add_classic("k0", 1, &["t1"], true);
    s.add_classic("k1", 0, &["t2"], true);
    for _ in 0..30 {
        s.tick();
    }
    let gone = s.clients.remove(0);
    let target = LeaveTarget {
        member_id: Some(id(&gone.name)),
        instance: None,
    };
    s.send(|t| Event::Leave(t, vec![target]));
    for _ in 0..30 {
        s.tick();
    }
    if join_a_member {
        // The targets are computed now; the group settles within 2 s, well inside the interval.
        s.add_classic("k2", 0, &["t3"], true);
        for _ in 0..4 {
            s.tick();
        }
    }
    assert!(s.clients[0].held.contains_key("t1"), "k0 holds t1");
    s.denied_bob.insert(1);
    s.set_rights();
    let (mut told, mut clean, base) = (None, None, s.rebalance_signals);
    for t in 0..200 {
        s.tick();
        if told.is_none() && s.rebalance_signals > base {
            told = Some(t);
        }
        if clean.is_none() && !s.clients[0].held.contains_key("t1") {
            clean = Some(t);
        }
    }
    (told, clean)
}

#[test]
fn a_classic_member_of_a_consumer_group_is_told_at_its_next_heartbeat_that_it_lost_a_right() {
    for assign_ms in [0, 1_000, 5_000] {
        for join_a_member in [false, true] {
            let (told, clean) = right_taken(assign_ms, join_a_member);
            assert_eq!(
                told,
                Some(0),
                "assign {assign_ms} joined {join_a_member}: told at the very next heartbeat"
            );
            assert!(
                clean.is_some_and(|t| t <= 3),
                "assign {assign_ms} joined {join_a_member}: the client stops holding t1 within 2 s, not {clean:?}"
            );
        }
    }
}

/// One right taken from six members of one principal (three of each protocol) and given back: the group's epoch moves
/// once per change for the members that hold the topic, not once per member, and the classic members are told.
#[test]
fn one_right_taken_and_given_back_moves_the_epoch_a_few_times() {
    for assign_ms in [0, 1_000] {
        let mut s = Sim::new(3, assign_ms);
        for k in 0..3 {
            s.add_cons(&format!("c{k}"), 1, &["t0", "t1", "t2"], None);
            s.add_classic(&format!("k{k}"), 1, &["t0", "t1", "t2"], false);
        }
        for _ in 0..30 {
            s.tick();
        }
        let (e0, signals0) = (s.view().epoch, s.rebalance_signals);
        s.denied_bob.insert(1);
        s.set_rights();
        for _ in 0..30 {
            s.tick();
        }
        let (e1, signals1) = (s.view().epoch, s.rebalance_signals);
        s.denied_bob.clear();
        s.set_rights();
        for _ in 0..30 {
            s.tick();
        }
        let e2 = s.view().epoch;
        assert!(
            signals1 > signals0,
            "assign {assign_ms}: the classic members are told"
        );
        assert!(
            (1..=8).contains(&(e1 - e0)) && (1..=8).contains(&(e2 - e1)),
            "assign {assign_ms}: epoch {e0} -> {e1} -> {e2}"
        );
    }
}
