//! Random heartbeats, leaves and time against a group, checking after every step what must always hold. A failing
//! run replays from its seed: `SIM_SEEDS=2000 cargo test -p kafka-broker --lib groups::consumer::tests::simulation`.

use super::*;

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) % n
    }
}

const NAMES: [&str; 4] = ["a", "b", "c", "d"];

fn check(r: &Rig, seed: u64, step: usize, last: &mut (i32, i32)) {
    let v = r.view();
    let ctx = format!("seed {seed} step {step}");
    // No partition is in two members' hands (a member holds what it is to give up until it says so).
    let mut held = Partitions::new();
    for m in &v.members {
        for p in &m.assigned {
            assert!(held.insert(*p), "{ctx}: {p:?} held twice");
        }
    }
    assert!(
        v.assignment_epoch <= v.epoch,
        "{ctx}: targets from the future"
    );
    assert!(
        v.epoch >= last.0 && v.assignment_epoch >= last.1,
        "{ctx}: an epoch went down"
    );
    *last = (v.epoch, v.assignment_epoch);
    for m in &v.members {
        assert!(
            m.epoch <= v.assignment_epoch,
            "{ctx}: {} ahead of the targets",
            m.id
        );
    }
}

#[test]
fn whatever_happens_no_partition_is_held_by_two_members_and_epochs_only_rise() {
    let seeds: u64 = std::env::var("SIM_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(150);
    for seed in 1..=seeds {
        let mut rng = Rng(seed);
        let mut r = rig(1 + rng.below(6) as i32);
        let mut epochs: BTreeMap<&str, i32> = BTreeMap::new();
        let mut last = (0, 0);
        for step in 0..120 {
            let name = NAMES[rng.below(4) as usize];
            match rng.below(10) {
                0..=1 => {
                    let a = r.beat(join(name));
                    if a.error.is_none() {
                        epochs.insert(name, a.epoch);
                    }
                }
                2..=6 => {
                    // A well-behaved member: reports what it holds minus what it was told to give up.
                    let epoch = epochs.get(name).copied().unwrap_or(2);
                    let owned: Vec<i32> = r
                        .view()
                        .members
                        .iter()
                        .find(|m| m.id.as_str() == name)
                        .map(|m| m.assigned.iter().map(|p| p.1).collect())
                        .unwrap_or_default();
                    let told = rng.below(2) == 0;
                    let h = if told {
                        poll_owning(name, epoch, &owned)
                    } else {
                        poll(name, epoch)
                    };
                    let a = r.beat(h);
                    if a.error.is_none() {
                        epochs.insert(name, a.epoch);
                    }
                }
                7 => {
                    let epoch = epochs.get(name).copied().unwrap_or(2);
                    r.beat(Heartbeat {
                        epoch: -1,
                        ..poll(name, epoch)
                    });
                    epochs.remove(name);
                }
                _ => r.advance(rng.below(4_000)),
            }
            check(&r, seed, step, &mut last);
        }
    }
}
