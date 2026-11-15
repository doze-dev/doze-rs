//! The Go map's columns: each app's depth is its longest chain of callers, so a request reads
//! left to right; a cycle is cut where it closes. The apps in no call at all are the lane. Then
//! each column is ordered so the lines cross little.

use std::collections::HashSet;

use crate::input::Flow;

/// The cards in the calls (apps that call or are called, then task cards by name), and the lane
/// (apps in no call at all), the most connected first.
pub(crate) fn split(f: &Flow<'_>) -> (Vec<usize>, Vec<usize>) {
    let mut nodes = Vec::new();
    let mut lane = Vec::new();
    for &a in &f.apps {
        if f.calls(a).is_empty() && f.callers(a).is_empty() {
            lane.push(a);
        } else {
            nodes.push(a);
        }
    }
    // The lane, the most connected first.
    lane.sort_by_key(|&a| std::cmp::Reverse(f.uses(a).len()));
    nodes.extend(f.carded_by_name());
    (nodes, lane)
}

/// The columns, each in its order, and the lane.
pub(crate) fn order(f: &Flow<'_>) -> (Vec<Vec<usize>>, Vec<usize>) {
    let (nodes, lane) = split(f);
    let back = back_edges(f, &nodes);
    let depth = depths(f, &nodes, &back);
    let mut cols: Vec<Vec<usize>> = Vec::new();
    for &v in &nodes {
        let d = depth.get(v).copied().unwrap_or(0);
        while cols.len() <= d {
            cols.push(Vec::new());
        }
        if let Some(c) = cols.get_mut(d) {
            c.push(v);
        }
    }
    (cols, lane)
}

/// The calls that close a cycle: a depth-first walk in display order, from the nodes nothing
/// calls first, drops the call that leads back to a node still on the walk.
pub(crate) fn back_edges(f: &Flow<'_>, nodes: &[usize]) -> HashSet<(usize, usize)> {
    let mut state = vec![0u8; f.ins.len()];
    let mut back = HashSet::new();
    let roots = nodes.iter().filter(|&&v| f.callers(v).is_empty());
    for &v in roots.chain(nodes.iter()) {
        if state.get(v) == Some(&0) {
            walk(f, v, &mut state, &mut back);
        }
    }
    back
}

/// One depth-first walk, with an explicit stack: a deep chain of calls cannot overflow it.
fn walk(f: &Flow<'_>, root: usize, state: &mut [u8], back: &mut HashSet<(usize, usize)>) {
    let mut stack = vec![(root, 0usize)];
    if let Some(s) = state.get_mut(root) {
        *s = 1;
    }
    while let Some((v, next)) = stack.pop() {
        let Some(&w) = f.calls(v).get(next) else {
            if let Some(s) = state.get_mut(v) {
                *s = 2;
            }
            continue;
        };
        stack.push((v, next + 1));
        match state.get(w) {
            Some(0) => {
                if let Some(s) = state.get_mut(w) {
                    *s = 1;
                }
                stack.push((w, 0));
            }
            Some(1) => {
                back.insert((v, w));
            }
            _ => {}
        }
    }
}

/// Each node's longest chain of callers, the cut calls aside.
pub(crate) fn depths(f: &Flow<'_>, nodes: &[usize], back: &HashSet<(usize, usize)>) -> Vec<usize> {
    let n = f.ins.len();
    let mut indeg = vec![0usize; n];
    for &v in nodes {
        for &w in f.calls(v) {
            if !back.contains(&(v, w))
                && let Some(d) = indeg.get_mut(w)
            {
                *d += 1;
            }
        }
    }
    let mut depth = vec![0usize; n];
    let mut ready: Vec<usize> = nodes
        .iter()
        .copied()
        .filter(|&v| indeg.get(v) == Some(&0))
        .collect();
    while let Some(v) = ready.pop() {
        let dv = depth.get(v).copied().unwrap_or(0);
        for &w in f.calls(v) {
            if back.contains(&(v, w)) {
                continue;
            }
            if let Some(dw) = depth.get_mut(w) {
                *dw = (*dw).max(dv + 1);
            }
            if let Some(d) = indeg.get_mut(w) {
                *d -= 1;
                if *d == 0 {
                    ready.push(w);
                }
            }
        }
    }
    depth
}

/// Orders each column by where its callers sit; where every card of a column hangs off the same
/// callers, the busiest goes in the middle and the rest alternate below and above, so the main
/// road runs straight. Gives each card its slot on one grid every column shares, and the grid's
/// number of rows.
pub(crate) fn arrange(f: &Flow<'_>, cols: &mut [Vec<usize>]) -> (Vec<Option<usize>>, usize) {
    let rows = cols.iter().map(Vec::len).max().unwrap_or(0);
    let mut slot: Vec<Option<usize>> = vec![None; f.ins.len()];
    let degree = |v: usize| f.calls(v).len() + f.callers(v).len();
    for c in cols.iter_mut() {
        let bary: Vec<f64> = c.iter().map(|&v| barycenter(f, &slot, v, rows)).collect();
        let same = bary.windows(2).all(|w| w.first() == w.last());
        let mut keyed: Vec<(f64, usize)> = bary.into_iter().zip(c.iter().copied()).collect();
        keyed.sort_by(|(ba, a), (bb, b)| {
            let by_bary = if same {
                std::cmp::Ordering::Equal
            } else {
                ba.total_cmp(bb)
            };
            by_bary
                .then_with(|| degree(*b).cmp(&degree(*a)))
                .then_with(|| f.name(*a).cmp(f.name(*b)))
        });
        *c = keyed.into_iter().map(|(_, v)| v).collect();
        let first = (rows - c.len()) / 2;
        if same && c.len() > 1 {
            *c = centre_out(c);
        }
        for (i, &v) in c.iter().enumerate() {
            if let Some(s) = slot.get_mut(v) {
                *s = Some(first + i);
            }
        }
    }
    (slot, rows)
}

fn barycenter(f: &Flow<'_>, slot: &[Option<usize>], v: usize, rows: usize) -> f64 {
    let placed: Vec<usize> = f
        .callers(v)
        .iter()
        .filter_map(|&u| slot.get(u).copied().flatten())
        .collect();
    if placed.is_empty() {
        #[allow(clippy::cast_precision_loss)]
        return (rows as f64 - 1.0) / 2.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let mean = placed.iter().map(|&s| s as f64).sum::<f64>() / placed.len() as f64;
    mean
}

/// The first in the middle, then below, then above.
fn centre_out(c: &[usize]) -> Vec<usize> {
    let mid = (c.len() - 1) / 2;
    let mut placed = vec![0usize; c.len()];
    for (i, &v) in c.iter().enumerate() {
        let off = i.div_ceil(2);
        let at = if i % 2 == 0 {
            mid.checked_sub(off)
        } else {
            Some(mid + off)
        };
        if let Some(p) = at.and_then(|a| placed.get_mut(a)) {
            *p = v;
        }
    }
    placed
}
