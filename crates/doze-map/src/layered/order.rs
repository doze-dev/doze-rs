//! Crossing reduction: each column ordered by the barycentre of its neighbours in the column
//! before it, sweeping right then left, keeping the order with the fewest crossings (counted
//! exactly, Barth–Jünger–Mutzel). Ties keep display order, which is what keeps a relayout stable.

use crate::layered::rank::Lg;

/// Sweeps at most this many times, and stops after `PATIENCE` sweeps that found nothing better.
const SWEEPS: usize = 12;
const PATIENCE: usize = 4;

/// Orders every column of `lg`. `seed` gives a vertex's place from an earlier layout (its y), which
/// the first order follows so a relayout keeps what it can.
pub(crate) fn reduce(lg: &mut Lg, seed: &dyn Fn(usize) -> Option<f64>) {
    if (0..lg.len()).any(|v| seed(v).is_some()) {
        // A relayout: what was placed keeps its order, what is new goes where its callers are.
        seeded(lg, seed);
        return;
    }
    initial(lg);
    let mut pos = positions(lg);
    let mut best = lg.layers.clone();
    let mut best_x = crossings(lg, &pos);
    let mut stale = 0;
    for i in 0..SWEEPS {
        if best_x == 0 || stale >= PATIENCE {
            break;
        }
        sweep(lg, &mut pos, i % 2 == 0);
        let x = crossings(lg, &pos);
        if x < best_x {
            (best_x, stale) = (x, 0);
            best.clone_from(&lg.layers);
        } else {
            stale += 1;
        }
    }
    lg.layers = best;
}

/// The first order: the first column in display order; each next column by the barycentre of its
/// callers' places, then display order.
fn initial(lg: &mut Lg) {
    let key = |lg: &Lg, pos: &[f64], v: usize| {
        let preds = lg.preds.get(v).map_or(&[][..], Vec::as_slice);
        mean(preds.iter().filter_map(|&u| pos.get(u).copied())).unwrap_or(0.0)
    };
    order_by(lg, &key, true);
}

/// A relayout's order: by the earlier layout's y; a vertex that has none (a new card, a line's
/// placeholder) at the mean y of its callers, so it lands beside them.
fn seeded(lg: &mut Lg, seed: &dyn Fn(usize) -> Option<f64>) {
    let key = |lg: &Lg, ys: &[f64], v: usize| {
        seed(v).unwrap_or_else(|| {
            let preds = lg.preds.get(v).map_or(&[][..], Vec::as_slice);
            mean(preds.iter().filter_map(|&u| ys.get(u).copied())).unwrap_or(f64::MAX)
        })
    };
    order_by(lg, &key, false);
}

/// Orders column after column by `key` (given the keys so far), ties by display order. With
/// `ranks`, a column's keys become its places (0, 1, 2 …) for the next; else they stay as given.
fn order_by(lg: &mut Lg, key: &dyn Fn(&Lg, &[f64], usize) -> f64, ranks: bool) {
    let mut keys = vec![0.0f64; lg.len()];
    for l in 0..lg.layers.len() {
        let Some(layer) = lg.layers.get(l) else {
            continue;
        };
        let mut keyed: Vec<(f64, usize, usize)> = layer
            .iter()
            .map(|&v| (key(lg, &keys, v), lg.model.get(v).copied().unwrap_or(v), v))
            .collect();
        keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for (i, k) in keyed.iter().enumerate() {
            if let Some(p) = keys.get_mut(k.2) {
                *p = if ranks { i as f64 } else { k.0 };
            }
        }
        if let Some(layer) = lg.layers.get_mut(l) {
            *layer = keyed.into_iter().map(|k| k.2).collect();
        }
    }
}

fn mean(it: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, n) = it.fold((0.0, 0u32), |(s, n), x| (s + x, n + 1));
    (n > 0).then(|| sum / f64::from(n))
}

/// Each vertex's index in its column.
fn positions(lg: &Lg) -> Vec<usize> {
    let mut pos = vec![0usize; lg.len()];
    for layer in &lg.layers {
        for (i, &v) in layer.iter().enumerate() {
            if let Some(p) = pos.get_mut(v) {
                *p = i;
            }
        }
    }
    pos
}

/// One sweep, right (`down`) or left: each column sorted by its neighbours' barycentre in the
/// column just ordered; a vertex with no neighbour there keeps its index.
fn sweep(lg: &mut Lg, pos: &mut [usize], down: bool) {
    let n = lg.layers.len();
    let order: Vec<usize> = if down {
        (1..n).collect()
    } else {
        (0..n.saturating_sub(1)).rev().collect()
    };
    for l in order {
        let Some(layer) = lg.layers.get(l) else {
            continue;
        };
        let nbrs = if down { &lg.preds } else { &lg.succs };
        let mut fixed: Vec<Option<usize>> = vec![None; layer.len()];
        let mut movable: Vec<(f64, usize, usize)> = Vec::new();
        for (i, &v) in layer.iter().enumerate() {
            let ns = nbrs.get(v).map_or(&[][..], Vec::as_slice);
            #[allow(clippy::cast_precision_loss)]
            match mean(ns.iter().filter_map(|&u| pos.get(u).map(|&p| p as f64))) {
                Some(b) => movable.push((b, i, v)),
                None => {
                    if let Some(s) = fixed.get_mut(i) {
                        *s = Some(v);
                    }
                }
            }
        }
        movable.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut it = movable.into_iter().map(|m| m.2);
        let new: Vec<usize> = fixed
            .into_iter()
            .filter_map(|f| f.or_else(|| it.next()))
            .collect();
        for (i, &v) in new.iter().enumerate() {
            if let Some(p) = pos.get_mut(v) {
                *p = i;
            }
        }
        if let Some(layer) = lg.layers.get_mut(l) {
            *layer = new;
        }
    }
}

/// Every crossing between neighbouring columns.
pub(crate) fn crossings(lg: &Lg, pos: &[usize]) -> u64 {
    lg.layers
        .iter()
        .zip(lg.layers.iter().skip(1))
        .map(|(a, b)| bilayer(lg, pos, a, b.len()))
        .sum()
}

/// Crossings between one column and the next, with an accumulator tree: the calls sorted by
/// their left end, then the inversions among their right ends.
fn bilayer(lg: &Lg, pos: &[usize], left: &[usize], right_len: usize) -> u64 {
    let mut ends: Vec<usize> = Vec::new();
    for &u in left {
        let mut mine: Vec<usize> = lg
            .succs
            .get(u)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .filter_map(|&v| pos.get(v).copied())
            .collect();
        mine.sort_unstable();
        ends.extend(mine);
    }
    let mut tree = vec![0u64; right_len + 1];
    let mut count = 0u64;
    for (seen, &e) in ends.iter().enumerate() {
        // How many earlier ends are at or before e; the rest cross.
        let mut i = e + 1;
        let mut le = 0u64;
        while i > 0 {
            le += tree.get(i).copied().unwrap_or(0);
            i &= i - 1;
        }
        count += seen as u64 - le;
        let mut i = e + 1;
        while i <= right_len {
            if let Some(t) = tree.get_mut(i) {
                *t += 1;
            }
            i += i.isolate_lowest_one();
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two calls that swap places cross once; a third parallel one adds nothing.
    #[test]
    fn counts_crossings_exactly() {
        let mut lg = Lg {
            real: vec![None; 6],
            layers: vec![vec![0, 1, 2], vec![3, 4, 5]],
            succs: vec![vec![4], vec![3], vec![5], vec![], vec![], vec![]],
            ..Lg::default()
        };
        let pos = positions(&lg);
        assert_eq!(crossings(&lg, &pos), 1);
        lg.succs = vec![vec![5], vec![4], vec![3], vec![], vec![], vec![]];
        assert_eq!(crossings(&lg, &pos), 3);
    }
}
