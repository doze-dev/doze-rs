//! The Map for 20 to 150 services: named regions past `classic_max` (and only with three of four
//! or more), the call-less folded into the tile, calls between regions marked, and the layout fast.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::time::Instant;

use doze_map::{CardId, Instance, Kind, Layout, Mode, Options, Rect, Role, layout, synth};

fn inside(outer: &Rect, r: &Rect) -> bool {
    r.x >= outer.x && r.y >= outer.y && r.right() <= outer.right() && r.bottom() <= outer.bottom()
}

fn flow_cards(l: &Layout) -> usize {
    (0..l.cards.len())
        .filter(|&i| !l.folded(CardId(i as u32)))
        .count()
}

#[test]
fn at_sixty_cards_or_fewer_the_map_is_the_go_map() {
    for n in [30, 60] {
        let ins = synth::medium(n, n * 2, 1);
        let l = layout(&ins, &Options::default());
        assert_eq!(l.mode, Mode::Classic, "{n}");
        assert!(l.regions.is_empty() && l.tile.is_none(), "{n}");
        assert!(l.edges.iter().all(|e| !e.across));
    }
}

#[test]
fn past_sixty_cards_the_map_is_by_region() {
    for (n, calls) in [(100, 190), (150, 300), (150, 500)] {
        let ins = synth::medium(n, calls, 1);
        let l = layout(&ins, &Options::default());
        assert_eq!(l.mode, Mode::Layered, "{n}");
        assert!(l.regions.len() >= 3, "{n}: {} regions", l.regions.len());
        // Frames apart, each holding its cards; every placed card in exactly one.
        for (i, a) in l.regions.iter().enumerate() {
            for b in l.regions.iter().skip(i + 1) {
                assert!(
                    !a.rect.intersects(&b.rect),
                    "{} and {} overlap",
                    a.name,
                    b.name
                );
            }
            for c in &a.cards {
                assert!(inside(&a.rect, &l.card(*c).unwrap().rect), "{}", a.name);
            }
        }
        let framed: usize = l.regions.iter().map(|r| r.cards.len()).sum();
        assert_eq!(framed, flow_cards(&l), "{n}");
        // A call is across exactly when its ends are in two regions.
        for e in &l.edges {
            let (a, b) = (l.region_of[e.from.ix()], l.region_of[e.to.ix()]);
            assert!(a.is_some() && b.is_some());
            assert_eq!(e.across, a != b, "{n}");
        }
        assert!(l.edges.iter().any(|e| e.across), "{n}: some calls cross");
        // Names, not cells: a region's cards share its stem (or are the rest).
        for r in &l.regions {
            if r.name != "other" {
                for c in &r.cards {
                    let name = &l.card(*c).unwrap().name;
                    assert!(
                        name.starts_with(&r.name) || name.starts_with("migrate-"),
                        "{name} in {}",
                        r.name
                    );
                }
            }
        }
        let o = l.overview();
        assert_eq!(o.regions.len(), l.regions.len());
        assert_eq!(
            o.bundles.iter().map(|b| b.count).sum::<u32>(),
            l.edges.iter().filter(|e| e.across).count() as u32
        );
    }
}

#[test]
fn the_call_less_fold_into_the_tile() {
    let ins = synth::medium(150, 300, 2);
    let l = layout(&ins, &Options::default());
    let tile = l.tile.as_ref().expect("a tile");
    let shared = ins
        .iter()
        .filter(|v| matches!(v.kind, Kind::Module | Kind::Service))
        .count();
    assert_eq!(tile.shared as usize, shared);
    assert!(tile.lane >= 1);
    assert_eq!(tile.cards.len(), (tile.lane + tile.shared) as usize);
    for &c in &tile.cards {
        assert!(l.folded(c));
        let card = l.card(c).unwrap();
        assert!(matches!(card.role, Role::Lane | Role::Shared));
        assert!(l.use_lines(c).is_empty(), "no band to draw use lines to");
    }
    // Not on the drawing (no hit, not in view), still found and still named.
    let all = l.visible(l.bounds);
    assert!(all.cards.iter().all(|c| !l.folded(*c)));
    let pg = l.card_named("postgres").unwrap();
    assert!(l.folded(pg));
    assert_eq!(l.search("postgres").first(), Some(&pg));
    assert!(!l.card(pg).unwrap().uses.is_empty(), "who uses it");
    // Off, the band and the lane are drawn as before.
    let opts = Options {
        regions: false,
        ..Options::default()
    };
    let plain = layout(&ins, &opts);
    assert!(plain.tile.is_none() && plain.regions.is_empty() && plain.band.is_some());
}

#[test]
fn no_naming_habit_keeps_the_layered_map_and_folds() {
    // 80 apps with no shared stems: no regions, but past sixty cards the call-less still fold.
    let mut ins: Vec<Instance> = (0..80)
        .map(|i| {
            let needs: Vec<String> = if i % 4 == 0 {
                Vec::new()
            } else {
                vec![format!("svc{}", i - 1)]
            };
            let mut v = Instance::new(&format!("svc{i}"), Kind::App, &[]);
            v.needs = needs;
            v
        })
        .collect();
    ins.push(Instance::new("lonely", Kind::App, &[]));
    let l = layout(&ins, &Options::default());
    assert_eq!(l.mode, Mode::Layered);
    assert!(l.regions.is_empty());
    assert_eq!(l.tile.as_ref().map(|t| t.lane), Some(1));
}

/// Layout at 150 well within a frame's budget (≤ 20 ms; release builds take about a millisecond).
#[test]
fn a_medium_stack_lays_out_fast() {
    let ins = synth::medium(150, 500, 3);
    let mut best = f64::MAX;
    for _ in 0..5 {
        let t = Instant::now();
        let l = layout(&ins, &Options::default());
        best = best.min(t.elapsed().as_secs_f64() * 1e3);
        assert!(l.regions.len() >= 3);
    }
    let budget = if cfg!(debug_assertions) { 200.0 } else { 20.0 };
    assert!(best <= budget, "{best:.2} ms");
    println!("medium 150/500 layout {best:.2} ms");
}

/// Between named regions the bundles run through the gaps: none passes over a third region's frame.
#[test]
fn bundles_keep_to_the_gaps_between_frames() {
    for (n, calls) in [(100, 190), (150, 300), (150, 500)] {
        let l = layout(&synth::medium(n, calls, 1), &Options::default());
        let t = Instant::now();
        let o = l.overview();
        let took = t.elapsed().as_secs_f64() * 1e3;
        // Timed in a release build only: a debug build on a CI runner is ten times slower and noisy.
        if !cfg!(debug_assertions) {
            assert!(took < 40.0, "{n}: {took:.1} ms");
            println!("overview {n}/{calls}: {took:.2} ms");
        }
        for b in &o.bundles {
            for s in &b.path.segs {
                let q = s.end();
                for (k, r) in l.regions.iter().enumerate() {
                    if k != b.from && k != b.to {
                        assert!(
                            !r.rect.inside(q),
                            "{n}: {} > {} over {}",
                            b.from,
                            b.to,
                            r.name
                        );
                    }
                }
            }
        }
    }
}
