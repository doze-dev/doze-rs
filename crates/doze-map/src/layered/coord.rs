//! Where each vertex sits down its column: as near the middle of its neighbours as the column's
//! order and spacing allow. Each column is one weighted isotonic regression (pool adjacent
//! violators, linear time), repeated sweeping right, left and both ways. Placeholders weigh more,
//! which pulls a long call straight.

use crate::layered::rank::Lg;

/// A card's height in the calls.
pub(crate) const CARD_H: f64 = 74.0;
const GAP_CARDS: f64 = 26.0;
const GAP_MIXED: f64 = 14.0;
const GAP_LINES: f64 = 10.0;
const ROUNDS: usize = 4;
/// A placeholder's pull, against a card's 1.
const LINE_WEIGHT: f64 = 3.0;
/// How strongly a vertex holds the place an earlier layout gave it, against a card.s 1.
const STICK: f64 = 4.0;

fn height(lg: &Lg, v: usize) -> f64 {
    if lg.real.get(v).copied().flatten().is_some() {
        CARD_H
    } else {
        0.0
    }
}

fn gap(lg: &Lg, a: usize, b: usize) -> f64 {
    let (ha, hb) = (height(lg, a), height(lg, b));
    let g = match (ha > 0.0, hb > 0.0) {
        (true, true) => GAP_CARDS,
        (false, false) => GAP_LINES,
        _ => GAP_MIXED,
    };
    (ha + hb) / 2.0 + g
}

/// Each vertex's centre y, the topmost card's top at 0. `seed` is an earlier layout's centre.
pub(crate) fn assign(lg: &Lg, seed: &dyn Fn(usize) -> Option<f64>) -> Vec<f64> {
    let mut y = vec![0.0f64; lg.len()];
    let seeded = (0..lg.len()).any(|v| seed(v).is_some());
    for layer in &lg.layers {
        let mut at = 0.0;
        let mut prev: Option<usize> = None;
        for &v in layer {
            at += prev.map_or(height(lg, v) / 2.0, |p| gap(lg, p, v));
            if let Some(s) = y.get_mut(v) {
                *s = seed(v).unwrap_or(at);
            }
            prev = Some(v);
        }
        solve(lg, layer, &mut y, &|_| None, seed, seeded);
    }
    let n = lg.layers.len();
    for _ in 0..ROUNDS {
        for l in 1..n {
            solve_layer(lg, l, &mut y, &[&lg.preds], seed, seeded);
        }
        for l in (0..n.saturating_sub(1)).rev() {
            solve_layer(lg, l, &mut y, &[&lg.succs], seed, seeded);
        }
    }
    for l in 0..n {
        solve_layer(lg, l, &mut y, &[&lg.preds, &lg.succs], seed, seeded);
    }
    let top = (0..lg.len())
        .map(|v| y.get(v).copied().unwrap_or(0.0) - height(lg, v) / 2.0)
        .fold(f64::INFINITY, f64::min);
    // A first layout starts at 0; a relayout keeps its frame, so what did not move stays put.
    let shift = if top.is_finite() && (!seeded || top < 0.0) {
        top
    } else {
        0.0
    };
    y.iter_mut().for_each(|c| *c = (*c - shift).round());
    y
}

fn solve_layer(
    lg: &Lg,
    l: usize,
    y: &mut [f64],
    sides: &[&Vec<Vec<usize>>],
    seed: &dyn Fn(usize) -> Option<f64>,
    seeded: bool,
) {
    let Some(layer) = lg.layers.get(l) else {
        return;
    };
    let snapshot: &[f64] = y;
    let targets: Vec<Option<f64>> = layer
        .iter()
        .map(|&v| {
            let ns = sides
                .iter()
                .flat_map(|s| s.get(v).map_or(&[][..], Vec::as_slice));
            let (sum, k) = ns.fold((0.0, 0.0), |(s, k), &u| {
                (s + snapshot.get(u).copied().unwrap_or(0.0), k + 1.0)
            });
            (k > 0.0).then(|| sum / k)
        })
        .collect();
    solve(
        lg,
        layer,
        y,
        &|i| targets.get(i).copied().flatten(),
        seed,
        seeded,
    );
}

/// Places one column: each vertex as near its target as the order and spacing allow, minimising
/// the weighted squared distance (weighted isotonic regression after taking the spacing out).
fn solve(
    lg: &Lg,
    layer: &[usize],
    y: &mut [f64],
    target: &dyn Fn(usize) -> Option<f64>,
    seed: &dyn Fn(usize) -> Option<f64>,
    floor: bool,
) {
    // offset[i]: the least distance from the column's first centre to the i-th's.
    let mut offset = Vec::with_capacity(layer.len());
    let mut acc = 0.0;
    for (i, &v) in layer.iter().enumerate() {
        if let Some(&p) = i.checked_sub(1).and_then(|j| layer.get(j)) {
            acc += gap(lg, p, v);
        }
        offset.push(acc);
    }
    // Blocks of pooled vertices: (weighted sum, weight, count).
    let mut blocks: Vec<(f64, f64, usize)> = Vec::with_capacity(layer.len());
    for (i, &v) in layer.iter().enumerate() {
        let cur = y.get(v).copied().unwrap_or(0.0);
        // A vertex an earlier layout placed stays there, unless what is new needs the room.
        let (t, w) = match (seed(v), target(i)) {
            (Some(s), _) => (s, STICK),
            (None, Some(t)) => (
                t,
                if height(lg, v) > 0.0 {
                    1.0
                } else {
                    LINE_WEIGHT
                },
            ),
            (None, None) => (cur, 0.25),
        };
        let z = t - offset.get(i).copied().unwrap_or(0.0);
        blocks.push((z * w, w, 1));
        while let [.., (s1, w1, n1), (s2, w2, n2)] = blocks.as_slice() {
            if s1 / w1 <= s2 / w2 {
                break;
            }
            let merged = (s1 + s2, w1 + w2, n1 + n2);
            blocks.truncate(blocks.len() - 2);
            blocks.push(merged);
        }
    }
    // A relayout keeps its frame: nothing new goes above the part's top, which would move it all.
    let least = if floor {
        layer.first().map_or(0.0, |&v| height(lg, v) / 2.0)
    } else {
        f64::MIN
    };
    let mut i = 0;
    for (s, w, n) in blocks {
        let z = (s / w).max(least);
        for _ in 0..n {
            if let (Some(&v), Some(o)) = (layer.get(i), offset.get(i))
                && let Some(c) = y.get_mut(v)
            {
                *c = z + o;
            }
            i += 1;
        }
    }
}
