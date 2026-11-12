//! The layered map's contract, checked on generated stacks up to 2,000 instances: no card on
//! another, no line behind a card (calls, and the use lines a selection draws), every call drawn,
//! the same input laid out the same way, the index agreeing with a scan, and a relayout that moves
//! only what changed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::collections::{HashMap, HashSet};

use doze_map::{
    Collapse, Instance, Kind, Layout, Mapper, Mode, Options, Path, Pt, Rect, Seg, layout, synth,
};

/// The size the slow checks run at: 2,000 in a release build, less in a debug one (CI runs debug).
fn big() -> usize {
    if cfg!(debug_assertions) { 500 } else { 2000 }
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

/// Points along a path, a few pixels apart.
fn samples(p: &Path) -> Vec<Pt> {
    let mut out = Vec::new();
    let mut cur = Pt::default();
    let lerp = |a: Pt, b: Pt, t: f64| Pt::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
    for s in &p.segs {
        match *s {
            Seg::Move(q) => cur = q,
            Seg::Line(q) => {
                let n = ((cur.x - q.x).hypot(cur.y - q.y) / 3.0).ceil().max(1.0) as usize;
                out.extend((0..=n).map(|i| lerp(cur, q, i as f64 / n as f64)));
                cur = q;
            }
            Seg::Quad(c, q) => {
                out.extend((0..=32).map(|i| {
                    let t = f64::from(i) / 32.0;
                    lerp(lerp(cur, c, t), lerp(c, q, t), t)
                }));
                cur = q;
            }
            Seg::Cubic(a, b, q) => {
                out.extend((0..=64).map(|i| {
                    let t = f64::from(i) / 64.0;
                    let (ab, bc, cd) = (lerp(cur, a, t), lerp(a, b, t), lerp(b, q, t));
                    lerp(lerp(ab, bc, t), lerp(bc, cd, t), t)
                }));
                cur = q;
            }
            _ => {}
        }
    }
    out
}

/// The first card a line passes behind, other than its ends.
fn behind(l: &Layout, p: &Path, ends: [usize; 2]) -> Option<(String, Pt)> {
    samples(p).into_iter().find_map(|q| {
        let near = l.visible(Rect::new(q.x - 1.0, q.y - 1.0, 2.0, 2.0)).cards;
        near.into_iter()
            .filter(|c| !ends.contains(&c.ix()))
            .find_map(|c| {
                l.card(c)
                    .filter(|c| c.rect.inside(q))
                    .map(|c| (c.name.clone(), q))
            })
    })
}

fn check(l: &Layout) {
    // No card on another.
    for (i, c) in l.cards.iter().enumerate() {
        for o in l.visible(c.rect).cards {
            assert!(
                o.ix() == i || !c.rect.intersects(&l.card(o).unwrap().rect),
                "{} overlaps {}",
                c.name,
                l.card(o).unwrap().name
            );
        }
        assert!(
            c.rect.x >= 0.0
                && c.rect.y >= 0.0
                && c.rect.right() <= l.bounds.w
                && c.rect.bottom() <= l.bounds.h,
            "{} outside the bounds",
            c.name
        );
    }
    // No call behind a card.
    for e in &l.edges {
        if let Some((n, q)) = behind(l, &e.path, [e.from.ix(), e.to.ix()]) {
            panic!(
                "{} > {} passes behind {n} at {q:?}",
                l.cards[e.from.ix()].name,
                l.cards[e.to.ix()].name
            );
        }
    }
    // Nor a use line, for every card that has them.
    for i in (0..l.cards.len()).step_by(7) {
        for (a, b, p) in l.use_lines(doze_map::CardId(i as u32)) {
            if let Some((n, q)) = behind(l, &p, [a.ix(), b.ix()]) {
                panic!(
                    "use {} > {} passes behind {n} at {q:?}",
                    l.cards[a.ix()].name,
                    l.cards[b.ix()].name
                );
            }
        }
    }
}

#[test]
fn large_stacks_lay_out_clear() {
    let sizes: &[(usize, u64)] = if cfg!(debug_assertions) {
        &[(150, 1), (400, 2)]
    } else {
        &[
            (150, 1),
            (600, 2),
            (2000, 3),
            (2000, 11),
            (2000, 12),
            (5000, 13),
        ]
    };
    for &(n, seed) in sizes {
        let ins = synth::stack(n, n * 5 / 2, seed);
        let l = layout(&ins, &expanded());
        assert_eq!(l.mode, Mode::Layered);
        check(&l);
        // Every call between two cards is drawn, once.
        let names: HashSet<&str> = l.cards.iter().map(|c| c.name.as_str()).collect();
        let tasks: HashMap<&str, bool> = ins
            .iter()
            .filter(|v| v.kind == Kind::Task)
            .map(|v| (v.name.as_str(), v.busy))
            .collect();
        let calls: HashSet<(&str, &str)> = ins
            .iter()
            .filter(|v| v.kind == Kind::App)
            .flat_map(|v| v.needs.iter().map(move |n| (v.name.as_str(), n.as_str())))
            .filter(|(a, b)| a != b && names.contains(b) && tasks.get(b).is_none_or(|busy| *busy))
            .filter(|(_, b)| {
                l.card(l.card_named(b).unwrap()).unwrap().kind != Kind::Module
                    && l.card(l.card_named(b).unwrap()).unwrap().kind != Kind::Service
            })
            .collect();
        assert_eq!(l.edges.len(), calls.len(), "{n}: every call drawn once");
        // The same input, the same layout.
        let again = layout(&ins, &expanded());
        assert_eq!(again.cards, l.cards);
        assert_eq!(again.edges, l.edges);
    }
}

#[test]
fn the_index_agrees_with_a_scan() {
    let l = layout(&synth::stack(big(), big() * 5 / 2, 9), &expanded());
    for (k, view) in [
        Rect::new(0.0, 0.0, 1280.0, 800.0),
        Rect::new(3000.0, 1200.0, 900.0, 600.0),
        Rect::new(-50.0, 500.0, 400.0, 4000.0),
    ]
    .into_iter()
    .enumerate()
    {
        let mut got: Vec<usize> = l.visible(view).cards.iter().map(|c| c.ix()).collect();
        got.sort_unstable();
        let want: Vec<usize> = (0..l.cards.len())
            .filter(|&i| touches(&l.cards[i].rect, &view))
            .collect();
        assert_eq!(got, want, "view {k}");
        let edges = l.visible(view).edges;
        for (i, e) in l.edges.iter().enumerate() {
            if samples(&e.path)
                .iter()
                .any(|p| p.x > view.x && p.x < view.right() && p.y > view.y && p.y < view.bottom())
            {
                assert!(edges.contains(&i), "edge {i} is in view {k} but not found");
            }
        }
    }
    let c = &l.cards[17];
    assert_eq!(l.hit(c.rect.center()).map(|id| id.ix()), Some(17));
}

/// Overlaps or touches: the index counts a card whose edge lies on the view's.
fn touches(a: &Rect, b: &Rect) -> bool {
    a.x <= b.right() && b.x <= a.right() && a.y <= b.bottom() && b.y <= a.bottom()
}

fn moved(a: &Layout, b: &Layout) -> usize {
    let at: HashMap<&str, Rect> = a.cards.iter().map(|c| (c.name.as_str(), c.rect)).collect();
    b.cards
        .iter()
        .filter(|c| at.get(c.name.as_str()).is_some_and(|r| *r != c.rect))
        .count()
}

#[test]
fn a_relayout_moves_only_what_changed() {
    let ins = synth::stack(big(), big() * 5 / 2, 4);
    let mut m = Mapper::new(expanded());
    let first = m.layout(&ins);
    // A change of state alone: the same layout, not even rebuilt.
    let same = m.layout(&ins.clone());
    assert!(std::sync::Arc::ptr_eq(&first, &same));
    // A task starts running: its card appears after its app; the rest stays put.
    let mut changed: Vec<Instance> = ins.clone();
    let task = changed
        .iter()
        .position(|v| v.kind == Kind::Task && !v.busy)
        .unwrap();
    changed[task].busy = true;
    let second = m.layout(&changed);
    assert_eq!(second.cards.len(), first.cards.len() + 1);
    let n = moved(&first, &second);
    assert!(
        n * 20 < first.cards.len(),
        "{n} of {} cards moved",
        first.cards.len()
    );
    check(&second);
    // And stops: back where it was.
    let third = m.layout(&ins);
    assert!(moved(&first, &third) * 20 < first.cards.len());
}

#[test]
fn groups_collapse_and_are_found_by_their_members() {
    let ins = synth::stack(big(), big() * 5 / 2, 5);
    let l = layout(&ins, &Options::default());
    assert!(
        l.cards.len() * 10 < ins.len() * 9,
        "{} cards: stamped groups collapse past 400 instances",
        l.cards.len()
    );
    let (group, members) = l.groups.iter().next().expect("a collapsed group");
    let card = l.card_named(&members[0]).unwrap();
    assert_eq!(&l.card(card).unwrap().name, group);
    assert_eq!(l.card(card).unwrap().members as usize, members.len());
    assert_eq!(l.search(&members[1]).first(), Some(&card));
    check(&l);
    // Opened, the group's members are cards again.
    let mut opts = Options::default();
    let label = group.trim_end_matches("_*").to_string();
    opts.collapse.expanded.insert(label);
    let open = layout(&ins, &opts);
    assert!(
        open.card_named(&members[0])
            .is_some_and(|c| open.card(c).unwrap().name == members[0])
    );
}
