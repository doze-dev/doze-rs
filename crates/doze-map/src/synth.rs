//! Synthetic stacks for benchmarks and for exercising a renderer: teams of services calling down
//! a few tiers, some stamped replicas (`team-worker_3`), a little calling across teams and back
//! up (cycles), apps in no call at all, modules and services used with a popular few, and setup
//! tasks, some running. Deterministic for a seed.

use std::collections::HashSet;

mod medium;
pub use medium::{medium, medium_trouble};

use crate::input::{Instance, Kind};

const WORDS: [&str; 16] = [
    "api",
    "web",
    "orders",
    "catalog",
    "gateway",
    "identity",
    "billing",
    "ledger",
    "search",
    "notify",
    "inventory",
    "fulfilment",
    "pricing",
    "reviews",
    "session",
    "media",
];
const TEAMS: [&str; 12] = [
    "payments", "shop", "growth", "core", "data", "platform", "mobile", "ads", "trust", "infra",
    "ml", "ops",
];
const ENGINES: [&str; 8] = [
    "postgres", "kafka", "redis", "nats", "mysql", "sqs", "minio", "elastic",
];

/// SplitMix64: small, fast, and the same everywhere.
#[derive(Debug, Clone)]
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            usize::try_from(self.next() % n as u64).unwrap_or(0)
        }
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> Option<&'a T> {
        let i = self.below(v.len());
        v.get(i)
    }
}

/// One team's apps by tier.
struct Team {
    tiers: Vec<Vec<usize>>,
}

/// A stack of about `nodes` instances with about `calls` calls between its apps, from `seed`. Uses
/// (one to three shared per app) come on top.
#[must_use]
pub fn stack(nodes: usize, calls: usize, seed: u64) -> Vec<Instance> {
    let mut r = Rng(seed);
    let n_shared = (nodes * 12 / 100).max(2);
    let n_tasks = nodes * 6 / 100;
    let n_apps = nodes.saturating_sub(n_shared + n_tasks).max(1);
    let n_lane = n_apps * 6 / 100;
    let mut apps: Vec<Instance> = Vec::with_capacity(n_apps);
    let teams = make_teams(&mut r, n_apps - n_lane, &mut apps);
    for i in 0..n_lane {
        let w = r.pick(&WORDS).copied().unwrap_or("app");
        apps.push(app(&format!("lane-{w}-{i}"), &mut r));
    }
    let shared: Vec<Instance> = (0..n_shared)
        .map(|i| {
            let kind = if i % 5 == 4 {
                Kind::Service
            } else {
                Kind::Module
            };
            let mut s = Instance::new(
                &format!("{}-{i}", r.pick(&ENGINES).copied().unwrap_or("db")),
                kind,
                &[],
            );
            s.engine = ENGINES
                .get(i % ENGINES.len())
                .copied()
                .unwrap_or("")
                .to_string();
            s
        })
        .collect();
    add_uses(&mut r, &mut apps, &shared);
    add_calls(&mut r, &mut apps, &teams, calls);
    let tasks = add_tasks(&mut r, &mut apps, n_tasks);
    let mut out = apps;
    out.sort_by(|a, b| a.name.cmp(&b.name));
    let mut rest: Vec<Instance> = shared.into_iter().chain(tasks).collect();
    rest.sort_by(|a, b| (a.kind as u8, &a.name).cmp(&(b.kind as u8, &b.name)));
    out.extend(rest);
    out
}

fn app(name: &str, r: &mut Rng) -> Instance {
    let mut a = Instance::new(name, Kind::App, &[]);
    a.run = [
        "go run .",
        "bun src/index.ts",
        "uv run app",
        "pnpm dev",
        "./bin/svc",
    ]
    .get(r.below(5))
    .copied()
    .unwrap_or("")
    .to_string();
    a
}

fn make_teams(r: &mut Rng, n: usize, apps: &mut Vec<Instance>) -> Vec<Team> {
    let mut teams = Vec::new();
    let mut left = n;
    while left > 0 {
        let size = (8 + r.below(110)).min(left);
        left -= size;
        let k = teams.len();
        let team = format!(
            "{}{}",
            TEAMS.get(k % TEAMS.len()).copied().unwrap_or("t"),
            k / TEAMS.len()
        );
        let depth = 3 + r.below(4);
        let mut tiers: Vec<Vec<usize>> = vec![Vec::new(); depth];
        let mut i = 0;
        while i < size {
            let tier = if i < 2 { 0 } else { 1 + r.below(depth - 1) };
            let word = r.pick(&WORDS).copied().unwrap_or("svc");
            // Some blocks stamp replicas: worker_0, worker_1 …
            let copies = if r.chance(8) {
                (3 + r.below(10)).min(size - i)
            } else {
                1
            };
            for c in 0..copies {
                let name = if copies > 1 {
                    format!("{team}-{word}{i}_{c}")
                } else {
                    format!("{team}-{word}-{i}")
                };
                if let Some(t) = tiers.get_mut(tier) {
                    t.push(apps.len());
                }
                apps.push(app(&name, r));
            }
            i += copies;
        }
        tiers.retain(|t| !t.is_empty());
        teams.push(Team { tiers });
    }
    teams
}

fn add_uses(r: &mut Rng, apps: &mut [Instance], shared: &[Instance]) {
    for a in apps.iter_mut() {
        let k = 1 + r.below(3);
        for _ in 0..k {
            // A popular few: the first shared are picked most.
            let i = r.below(shared.len()).min(r.below(shared.len()));
            if let Some(s) = shared.get(i)
                && !a.needs.contains(&s.name)
            {
                a.needs.push(s.name.clone());
            }
        }
    }
}

fn add_calls(r: &mut Rng, apps: &mut [Instance], teams: &[Team], budget: usize) {
    let mut have: HashSet<(usize, usize)> = HashSet::new();
    let mut call = |apps: &mut [Instance], a: usize, b: usize| {
        if a == b || !have.insert((a, b)) {
            return false;
        }
        let name = apps.get(b).map(|x| x.name.clone()).unwrap_or_default();
        apps.get_mut(a).is_some_and(|x| {
            x.needs.push(name);
            true
        })
    };
    let mut made = 0;
    // Every app below the first tier has a caller in the tier above: each team is one part.
    for t in teams {
        for (above, tier) in t.tiers.iter().zip(t.tiers.iter().skip(1)) {
            for &b in tier {
                if let Some(&a) = r.pick(above)
                    && call(apps, a, b)
                {
                    made += 1;
                }
            }
        }
    }
    let mut tries = 0;
    while made < budget && tries < budget * 20 {
        tries += 1;
        let Some(t) = r.pick(teams) else { break };
        let ti = r.below(t.tiers.len().saturating_sub(1));
        let Some(&a) = t.tiers.get(ti).and_then(|tier| r.pick(tier)) else {
            continue;
        };
        let roll = r.below(100);
        let to_tier = match roll {
            0..80 => ti + 1,
            80..96 => ti + 2 + r.below(2),
            _ => r.below(ti + 1), // back up: a cycle
        };
        let target_team = if r.chance(3) {
            r.pick(teams).unwrap_or(t)
        } else {
            t
        };
        let tier = target_team
            .tiers
            .get(to_tier.min(target_team.tiers.len().saturating_sub(1)));
        if let Some(&b) = tier.and_then(|tier| r.pick(tier))
            && call(apps, a, b)
        {
            made += 1;
        }
    }
}

fn add_tasks(r: &mut Rng, apps: &mut [Instance], n: usize) -> Vec<Instance> {
    (0..n)
        .map(|i| {
            let mut t = Instance::new(&format!("migrate-{i}"), Kind::Task, &[]);
            t.busy = r.chance(10);
            let k = r.below(apps.len());
            if let Some(a) = apps.get_mut(k) {
                a.needs.push(t.name.clone());
            }
            t
        })
        .collect()
}
