//! Medium stacks (20 to 150 services) shaped like a real one: a few teams' services named for
//! their team (`checkout-api`, `checkout-cart` …), each team's front door called by a `gateway`,
//! calls down a few tiers inside a team and a little across, shared modules everyone uses
//! (postgres, redis, kafka …) and a database or two of a team's own, apps nothing calls (a docs
//! site, a team's cron), setup tasks, one of them failed. Deterministic for a seed.

use std::collections::HashSet;

use super::Rng;
use crate::input::{Instance, Kind};

const TEAMS: [&str; 8] = [
    "checkout", "catalog", "accounts", "payments", "shipping", "search", "notify", "media",
];
const WORDS: [&str; 40] = [
    "cart",
    "pricing",
    "rules",
    "events",
    "sync",
    "export",
    "scheduler",
    "ledger",
    "risk",
    "index",
    "ingest",
    "thumbs",
    "admin",
    "quotes",
    "labels",
    "tracking",
    "feed",
    "reviews",
    "promo",
    "tax",
    "refunds",
    "sessions",
    "profile",
    "mfa",
    "login",
    "emails",
    "sms",
    "push",
    "digest",
    "rates",
    "returns",
    "stock",
    "suggest",
    "ranker",
    "upload",
    "resize",
    "audit",
    "fraud",
    "payouts",
    "worker",
];
/// Shared modules and services: name, engine, kind.
const SHARED: [(&str, &str, Kind); 12] = [
    ("postgres", "postgres", Kind::Module),
    ("redis", "redis", Kind::Module),
    ("kafka", "kafka", Kind::Module),
    ("minio", "minio", Kind::Module),
    ("elastic", "elastic", Kind::Module),
    ("mailpit", "mailpit", Kind::Service),
    ("nats", "nats", Kind::Module),
    ("stripe-mock", "", Kind::Service),
    ("clickhouse", "clickhouse", Kind::Module),
    ("mysql", "mysql", Kind::Module),
    ("rabbitmq", "rabbitmq", Kind::Module),
    ("temporal", "temporal", Kind::Module),
];
const RUNS: [&str; 5] = [
    "go run ./cmd/svc",
    "bun src/index.ts",
    "uv run app",
    "pnpm dev",
    "./bin/svc",
];

/// A medium stack of about `n` instances with about `calls` calls between its apps.
#[must_use]
pub fn medium(n: usize, calls: usize, seed: u64) -> Vec<Instance> {
    let mut r = Rng(seed);
    let n_shared = (n / 10).clamp(4, SHARED.len());
    let n_tasks = (n / 25).max(1);
    let n_lane = (n * 6 / 100).max(1);
    let n_teams = ((n + 10) / 20).clamp(3, TEAMS.len());
    let n_team_apps = n
        .saturating_sub(n_shared + n_tasks + n_lane + 2)
        .max(n_teams * 3);
    let mut apps: Vec<Instance> = Vec::new();
    let teams = make_teams(&mut r, n_teams, n_team_apps, &mut apps);
    let mut have: HashSet<(usize, usize)> = HashSet::new();
    let mut made = 0usize;
    // The gateway calls each team's front door; an admin console a few of them.
    let gw = apps.len();
    apps.push(app("gateway", &mut r));
    for t in &teams {
        made += usize::from(call(
            &mut apps,
            &mut have,
            gw,
            t.first().copied().unwrap_or(0),
        ));
    }
    let admin = apps.len();
    apps.push(app("admin-ui", &mut r));
    for t in teams.iter().take(3) {
        made += usize::from(call(
            &mut apps,
            &mut have,
            admin,
            t.first().copied().unwrap_or(0),
        ));
    }
    made += tiers(&mut r, &mut apps, &mut have, &teams);
    more_calls(
        &mut r,
        &mut apps,
        &mut have,
        &teams,
        calls.saturating_sub(made),
    );
    for i in 0..n_lane {
        let name = match i {
            0 => "docs-site".to_string(),
            1 => "storybook".to_string(),
            _ => format!("{}-cron", TEAMS.get(i - 2).copied().unwrap_or("ops")),
        };
        apps.push(app(&name, &mut r));
    }
    let shared = shared(n_shared, &teams, &apps);
    uses(&mut r, &mut apps, &shared, &teams);
    let tasks = tasks(&mut apps, &teams, n_tasks);
    apps.extend(shared);
    apps.extend(tasks);
    apps
}

/// The instances a fixture shows in trouble: the second team's front door and something it calls
/// (so the trouble has a path between), the last team's last app, and redis.
#[must_use]
pub fn medium_trouble(ins: &[Instance]) -> Vec<String> {
    let doors: Vec<&Instance> = ins
        .iter()
        .filter(|v| v.kind == Kind::App && v.name.ends_with("-api"))
        .collect();
    let mut out = Vec::new();
    if let Some(door) = doors.get(1) {
        out.push(door.name.clone());
        let team = door.name.trim_end_matches("api");
        out.extend(door.needs.iter().find(|n| n.starts_with(team)).cloned());
    }
    if let Some(last) = doors.last() {
        let team = last.name.trim_end_matches("api");
        out.extend(
            ins.iter()
                .rev()
                .find(|v| v.kind == Kind::App && v.name.starts_with(team) && v.name != last.name)
                .map(|v| v.name.clone()),
        );
    }
    out.extend(
        ins.iter()
            .find(|v| v.name == "redis")
            .map(|v| v.name.clone()),
    );
    out
}

fn app(name: &str, r: &mut Rng) -> Instance {
    let mut a = Instance::new(name, Kind::App, &[]);
    a.run = RUNS
        .get(r.below(RUNS.len()))
        .copied()
        .unwrap_or("")
        .to_string();
    a
}

fn call(apps: &mut [Instance], have: &mut HashSet<(usize, usize)>, a: usize, b: usize) -> bool {
    if a == b || !have.insert((a, b)) {
        return false;
    }
    let name = apps.get(b).map(|x| x.name.clone()).unwrap_or_default();
    apps.get_mut(a).is_some_and(|x| {
        x.needs.push(name);
        true
    })
}

/// Each team's apps by index, its front door (`team-api`) first; sizes vary by team.
fn make_teams(r: &mut Rng, n: usize, total: usize, apps: &mut Vec<Instance>) -> Vec<Vec<usize>> {
    let weights = [14usize, 12, 11, 10, 9, 8, 7, 6];
    let sum: usize = weights.iter().take(n).sum();
    let mut teams = Vec::with_capacity(n);
    let mut words: Vec<&str> = WORDS.to_vec();
    for k in 0..n {
        let team = TEAMS.get(k).copied().unwrap_or("team");
        let size = (total * weights.get(k).copied().unwrap_or(6) / sum.max(1)).max(3);
        let mut ids = vec![apps.len()];
        apps.push(app(&format!("{team}-api"), r));
        // A word once per team; worker last, as most teams have one.
        let start = r.below(words.len());
        words.rotate_left(start);
        for i in 1..size {
            let w = words.get(i - 1).copied().unwrap_or("svc");
            let name = if i + 1 == size && size > 4 {
                format!("{team}-worker")
            } else {
                format!("{team}-{w}")
            };
            if apps.iter().any(|a| a.name == name) {
                continue;
            }
            ids.push(apps.len());
            apps.push(app(&name, r));
        }
        teams.push(ids);
    }
    teams
}

/// Inside each team: the front door calls the first tier, each later app has a caller in the tier
/// before it.
fn tiers(
    r: &mut Rng,
    apps: &mut [Instance],
    have: &mut HashSet<(usize, usize)>,
    teams: &[Vec<usize>],
) -> usize {
    let mut made = 0;
    for t in teams {
        let Some((&api, rest)) = t.split_first() else {
            continue;
        };
        for (i, &b) in rest.iter().enumerate() {
            let tier = tier_of(i + 1, t.len());
            // A caller one tier up.
            let above: Vec<usize> = (0..=i)
                .filter(|&j| tier_of(j, t.len()) + 1 == tier)
                .collect();
            let a = above
                .get(r.below(above.len()))
                .and_then(|&j| t.get(j))
                .copied()
                .unwrap_or(api);
            made += usize::from(call(apps, have, a, b));
        }
    }
    made
}

/// A team member's tier by its place in the team (the front door first): the front door, then about a third,
/// then most of the rest, then the last few.
fn tier_of(pos: usize, len: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    let n = len.saturating_sub(1).max(1);
    let first = n.div_ceil(3);
    let second = first + (n - first.min(n)) * 3 / 5;
    match pos - 1 {
        p if p < first => 1,
        p if p < second.max(first + 1) => 2,
        _ => 3,
    }
}

/// More calls, mostly down inside a team, some across to another team's front door or worker.
fn more_calls(
    r: &mut Rng,
    apps: &mut [Instance],
    have: &mut HashSet<(usize, usize)>,
    teams: &[Vec<usize>],
    budget: usize,
) {
    let mut made = 0;
    let mut tries = 0;
    while made < budget && tries < budget * 30 {
        tries += 1;
        let ti = r.below(teams.len());
        let Some(t) = teams.get(ti) else { break };
        let i = r.below(t.len());
        let Some(&a) = t.get(i) else { continue };
        let b = if r.chance(14) {
            // Across: a later team's front door, or now and then its worker.
            let other = teams.get((ti + 1 + r.below(teams.len() - 1)) % teams.len());
            other
                .and_then(|o| if r.chance(70) { o.first() } else { o.last() })
                .copied()
        } else {
            // Down: to a later app of a deeper tier, so a team stays three or four tiers deep.
            let deeper: Vec<usize> = (i + 1..t.len())
                .filter(|&j| tier_of(j, t.len()) > tier_of(i, t.len()))
                .collect();
            deeper
                .get(r.below(deeper.len()))
                .and_then(|&j| t.get(j))
                .copied()
        };
        if let Some(b) = b
            && call(apps, have, a, b)
        {
            made += 1;
        }
    }
}

fn shared(n: usize, teams: &[Vec<usize>], apps: &[Instance]) -> Vec<Instance> {
    let mut out: Vec<Instance> = SHARED
        .iter()
        .take(n)
        .map(|(name, engine, kind)| {
            let mut s = Instance::new(name, *kind, &[]);
            s.engine = (*engine).to_string();
            s
        })
        .collect();
    // The two largest teams keep a database of their own.
    for t in teams.iter().take(2) {
        let team = t
            .first()
            .and_then(|&a| apps.get(a))
            .and_then(|a| a.name.split('-').next())
            .unwrap_or("team");
        let mut s = Instance::new(&format!("{team}-db"), Kind::Module, &[]);
        s.engine = "postgres".into();
        out.push(s);
    }
    out
}

/// One or two shared each: postgres, redis and kafka most; a team's own database for its front door.
fn uses(r: &mut Rng, apps: &mut [Instance], shared: &[Instance], teams: &[Vec<usize>]) {
    let common = shared.len().saturating_sub(2).max(1);
    for a in apps.iter_mut() {
        for _ in 0..1 + r.below(2) {
            let i = r.below(common).min(r.below(common));
            if let Some(s) = shared.get(i)
                && !a.needs.contains(&s.name)
            {
                a.needs.push(s.name.clone());
            }
        }
    }
    for (k, t) in teams.iter().take(2).enumerate() {
        let db = shared.get(shared.len() - 2 + k).map(|s| s.name.clone());
        for &v in t.iter().take(3) {
            if let (Some(db), Some(a)) = (db.clone(), apps.get_mut(v)) {
                a.needs.push(db);
            }
        }
    }
}

/// A migration per team, needed by its front door; the second one failed (a card of its own).
fn tasks(apps: &mut [Instance], teams: &[Vec<usize>], n: usize) -> Vec<Instance> {
    teams
        .iter()
        .take(n)
        .enumerate()
        .filter_map(|(k, t)| {
            let api = apps.get_mut(*t.first()?)?;
            let team = api.name.split('-').next().unwrap_or("team").to_string();
            let mut task = Instance::new(&format!("migrate-{team}"), Kind::Task, &[]);
            task.busy = k == 1;
            api.needs.push(task.name.clone());
            Some(task)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{medium, medium_trouble};
    use crate::input::Kind;

    #[test]
    fn medium_stacks_have_the_asked_shape() {
        for (n, calls) in [(30, 50), (60, 110), (100, 190), (150, 300), (150, 500)] {
            let ins = medium(n, calls, 1);
            assert!(
                ins.len() + 8 >= n && ins.len() <= n + 8,
                "{n}: {} instances",
                ins.len()
            );
            let names: std::collections::HashSet<&str> =
                ins.iter().map(|v| v.name.as_str()).collect();
            assert_eq!(names.len(), ins.len(), "{n}: unique names");
            let app_calls: usize = ins
                .iter()
                .filter(|v| v.kind == Kind::App)
                .flat_map(|v| v.needs.iter())
                .filter(|need| ins.iter().any(|w| &w.name == *need && w.kind == Kind::App))
                .count();
            assert!(
                app_calls * 10 >= calls * 7,
                "{n}: {app_calls} calls for {calls} asked"
            );
            assert!(names.contains("postgres") && names.contains("redis"));
            assert!(medium_trouble(&ins).len() >= 3, "{n}: some trouble");
        }
        assert_eq!(medium(100, 190, 3), medium(100, 190, 3));
    }
}
