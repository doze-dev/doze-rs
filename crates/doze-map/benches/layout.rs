//! The Map's budgets, measured: `cargo bench -p doze-map`. Prints a table (medians) and fails
//! when a budget is broken. Stacks come from `doze_map::synth`, edges about 2.5 per node.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::collections::HashMap;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use doze_map::{Collapse, Instance, Kind, Layout, Mapper, Options, Rect, layout, synth};

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v.get(v.len() / 2).copied().unwrap_or_default()
}

fn time<T>(runs: usize, mut f: impl FnMut() -> T) -> Duration {
    median(
        (0..runs)
            .map(|_| {
                let t = Instant::now();
                std::hint::black_box(f());
                t.elapsed()
            })
            .collect(),
    )
}

fn ms(d: Duration) -> String {
    if d < Duration::from_millis(1) {
        format!("{:.1} µs", d.as_secs_f64() * 1e6)
    } else {
        format!("{:.2} ms", d.as_secs_f64() * 1e3)
    }
}

fn expanded() -> Options {
    Options {
        collapse: Collapse {
            over: usize::MAX,
            ..Collapse::default()
        },
        ..Options::default()
    }
}

/// The stack with one more task running (a card appears beside its app), and that app's name.
fn one_task_runs(ins: &[Instance]) -> Option<(Vec<Instance>, String)> {
    let mut out = ins.to_vec();
    let task = out.iter().position(|v| v.kind == Kind::Task && !v.busy)?;
    let name = out.get(task)?.name.clone();
    let app = out.iter().find(|v| v.needs.contains(&name))?.name.clone();
    out.get_mut(task)?.busy = true;
    Some((out, app))
}

struct Row {
    nodes: usize,
    edges: usize,
    full: Duration,
    mode: String,
    cards: usize,
    query: Duration,
    whole: Duration,
    state: Duration,
    part: Duration,
    moved: String,
}

/// A window's worth of map (1280 × 800 at zoom 1) centred on a card: what a user looks at.
fn views(l: &Layout) -> Vec<Rect> {
    let mut seed = 1u64;
    (0..1000)
        .map(|_| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let c = l
                .cards
                .get((seed >> 33) as usize % l.cards.len().max(1))
                .map_or(Rect::default(), |c| c.rect);
            Rect::new(c.center().x - 640.0, c.center().y - 400.0, 1280.0, 800.0)
        })
        .collect()
}

/// A relayout after one task starts running: its time (only the relayout, after a first layout of
/// the stack as it was), and how many cards it moved.
fn one_part(ins: &[Instance], opts: &Options, runs: usize) -> (Duration, String) {
    let Some((changed, _app)) = one_task_runs(ins) else {
        return (Duration::ZERO, "-".into());
    };
    let d = median(
        (0..runs)
            .map(|_| {
                let mut m = Mapper::new(opts.clone());
                let _ = m.layout(ins);
                let t = Instant::now();
                std::hint::black_box(m.layout(&changed));
                t.elapsed()
            })
            .collect(),
    );
    let mut m = Mapper::new(opts.clone());
    let before: HashMap<String, Rect> = m
        .layout(ins)
        .cards
        .iter()
        .map(|c| (c.name.clone(), c.rect))
        .collect();
    let after = m.layout(&changed);
    let moved = after
        .cards
        .iter()
        .filter(|c| before.get(&c.name).is_some_and(|r| *r != c.rect))
        .count();
    (d, format!("{moved} of {}", before.len()))
}

/// A row: `synth::stack`, or with `medium` a medium stack shaped like a real one (`synth::medium`, named
/// regions) with about 500 calls at 150.
fn row(n: usize, opts: &Options, medium: bool) -> Row {
    let (e, ins) = if medium {
        (n * 10 / 3, synth::medium(n, n * 10 / 3, 1))
    } else {
        (n * 5 / 2, synth::stack(n, n * 5 / 2, 7))
    };
    let runs = if n >= 2000 { 7 } else { 21 };
    let full = time(runs, || layout(&ins, opts));
    let l = layout(&ins, opts);
    let views = views(&l);
    let mut vi = views.iter().cycle();
    let query = time(1000, || l.visible(*vi.next().unwrap_or(&Rect::default())));
    let whole = time(101, || l.visible(l.bounds));
    let seen: usize = views
        .iter()
        .map(|v| l.visible(*v).cards.len() + l.visible(*v).edges.len())
        .sum();
    let all = l.visible(l.bounds);
    eprintln!(
        "{n}: drawing {:.0} × {:.0}; a viewport holds {:.1} cards and lines on average; the whole map {} cards, {} lines",
        l.bounds.w,
        l.bounds.h,
        seen as f64 / views.len() as f64,
        all.cards.len(),
        all.edges.len()
    );
    let mut m = Mapper::new(opts.clone());
    let _ = m.layout(&ins);
    let state = time(runs, || m.layout(&ins));
    let (part, moved) = one_part(&ins, opts, runs);
    Row {
        nodes: n,
        edges: e,
        full,
        mode: format!("{:?}", l.mode),
        cards: l.cards.len(),
        query,
        whole,
        state,
        part,
        moved,
    }
}

fn main() -> ExitCode {
    let mut ok = true;
    println!(
        "| nodes / edges | cards | mode | full layout | viewport query (1280×800) | whole-map query |  state change | one task card appears | cards moved |"
    );
    println!("|---|---|---|---|---|---|---|---|---|");
    let sizes: &[(usize, bool, bool)] = &[
        (10, false, false),
        (100, false, false),
        (100, false, true),
        (150, false, true),
        (1000, false, false),
        (2000, false, false),
        (5000, false, false),
        (2000, true, false),
        (5000, true, false),
    ];
    for &(n, collapse, medium) in sizes {
        let opts = if collapse {
            Options::default()
        } else {
            expanded()
        };
        let r = row(n, &opts, medium);
        let tag = match (collapse, medium) {
            (true, _) => " (groups collapsed)",
            (_, true) => " (medium, by region)",
            _ => "",
        };
        println!(
            "| {} / {}{tag} | {} | {} | {} | {} | {} | {} | {} | {} |",
            r.nodes,
            r.edges,
            r.cards,
            r.mode,
            ms(r.full),
            ms(r.query),
            ms(r.whole),
            ms(r.state),
            ms(r.part.max(Duration::ZERO)),
            r.moved
        );
        if r.nodes == 150 && medium {
            ok &= check("full layout, 150 services", r.full, 20.0);
        }
        if r.nodes == 2000 && !collapse {
            ok &= check("full layout, 2,000 nodes", r.full, 50.0);
            ok &= check("viewport query", r.query, 1.0);
            ok &= check("state change", r.state, 5.0);
            ok &= check("one part changed", r.part, 20.0);
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn check(what: &str, d: Duration, budget_ms: f64) -> bool {
    let fine = d.as_secs_f64() * 1e3 <= budget_ms;
    if !fine {
        eprintln!(
            "over budget: {what} took {}, the budget is {budget_ms} ms",
            ms(d)
        );
    }
    fine
}
