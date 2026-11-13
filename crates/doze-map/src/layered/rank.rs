//! A connected part of the calls as a layered graph: each card in the column of its longest chain
//! of callers (the Go map's rule), and a call that skips columns broken into a chain through
//! placeholder points, one per column it crosses, so it has a reserved place in each and never
//! passes behind a card there.

use std::collections::{HashMap, HashSet};

use crate::input::Flow;

/// One call as a chain of vertices, lowest column first.
#[derive(Debug, Clone)]
pub(crate) struct Chain {
    /// The caller and the called instance.
    pub(crate) from: usize,
    pub(crate) to: usize,
    /// Vertices from the lower column to the higher, the cards at the two ends.
    pub(crate) verts: Vec<usize>,
    /// It closes a cycle: the called sits left of the caller.
    pub(crate) back: bool,
}

/// The layered graph of one part.
#[derive(Debug, Clone, Default)]
pub(crate) struct Lg {
    /// Vertex to its instance; `None` for a placeholder.
    pub(crate) real: Vec<Option<usize>>,
    /// Vertex to the order a tie is broken by: the instance's display order, or for a
    /// placeholder its call's caller's.
    pub(crate) model: Vec<usize>,
    pub(crate) layer: Vec<usize>,
    /// The vertices one column left and right that a call joins it to.
    pub(crate) preds: Vec<Vec<usize>>,
    pub(crate) succs: Vec<Vec<usize>>,
    /// Each column's vertices, in order top to bottom.
    pub(crate) layers: Vec<Vec<usize>>,
    pub(crate) chains: Vec<Chain>,
}

impl Lg {
    pub(crate) fn len(&self) -> usize {
        self.real.len()
    }

    fn add(&mut self, real: Option<usize>, model: usize, layer: usize) -> usize {
        let v = self.real.len();
        self.real.push(real);
        self.model.push(model);
        self.layer.push(layer);
        self.preds.push(Vec::new());
        self.succs.push(Vec::new());
        while self.layers.len() <= layer {
            self.layers.push(Vec::new());
        }
        if let Some(l) = self.layers.get_mut(layer) {
            l.push(v);
        }
        v
    }

    fn join(&mut self, a: usize, b: usize) {
        if let Some(s) = self.succs.get_mut(a) {
            if s.contains(&b) {
                return;
            }
            s.push(b);
        }
        if let Some(p) = self.preds.get_mut(b) {
            p.push(a);
        }
    }
}

/// The layered graph of the part `members` (instances, in display order), given every node's
/// depth and the calls cut to break cycles.
pub(crate) fn build(
    f: &Flow<'_>,
    members: &[usize],
    depth: &[usize],
    back: &HashSet<(usize, usize)>,
) -> Lg {
    let base = members
        .iter()
        .filter_map(|&v| depth.get(v))
        .copied()
        .min()
        .unwrap_or(0);
    let layer_of = |v: usize| depth.get(v).copied().unwrap_or(base) - base;
    let mut lg = Lg::default();
    let mut vid: HashMap<usize, usize> = HashMap::with_capacity(members.len());
    for &v in members {
        vid.insert(v, lg.add(Some(v), v, layer_of(v)));
    }
    // A caller's long calls share a trunk: one placeholder per column they cross, by caller,
    // column and direction; each leaves the trunk at the column before its target.
    let mut trunk: HashMap<(usize, usize, bool), usize> = HashMap::new();
    for &u in members {
        for &w in f.calls(u) {
            let (Some(&a), Some(&b)) = (vid.get(&u), vid.get(&w)) else {
                continue;
            };
            let is_back = back.contains(&(u, w));
            let (lo, hi) = if is_back { (b, a) } else { (a, b) };
            let (llo, lhi) = (
                lg.layer.get(lo).copied().unwrap_or(0),
                lg.layer.get(hi).copied().unwrap_or(0),
            );
            let mut verts = vec![lo];
            for l in llo + 1..lhi {
                let d = *trunk
                    .entry((u, l, is_back))
                    .or_insert_with(|| lg.add(None, u, l));
                verts.push(d);
            }
            verts.push(hi);
            for pair in verts.windows(2) {
                if let [x, y] = *pair {
                    lg.join(x, y);
                }
            }
            lg.chains.push(Chain {
                from: u,
                to: w,
                verts,
                back: is_back,
            });
        }
    }
    lg
}

/// Moves cards right, toward what they call, where that shortens more lines than it stretches:
/// an app nothing calls sits just before the first thing it calls rather than at the far left,
/// so its calls need no placeholders. Callers stay left of what they call. (The Go map keeps
/// every uncalled app in the first column; at scale that costs thousands of placeholders.)
pub(crate) fn promote(
    f: &Flow<'_>,
    nodes: &[usize],
    back: &HashSet<(usize, usize)>,
    depth: &mut [usize],
) {
    let fwd = |v: usize| {
        f.calls(v)
            .iter()
            .copied()
            .filter(move |&w| !back.contains(&(v, w)))
    };
    let ins = |v: usize| {
        f.callers(v)
            .iter()
            .filter(|&&u| !back.contains(&(u, v)))
            .count()
    };
    // Deepest first: what a card calls is final before the card moves.
    let mut by_depth: Vec<usize> = nodes.to_vec();
    by_depth.sort_by_key(|&v| std::cmp::Reverse(depth.get(v).copied().unwrap_or(0)));
    for v in by_depth {
        let outs = fwd(v).count();
        if outs == 0 || outs <= ins(v) {
            continue;
        }
        let Some(least) = fwd(v).filter_map(|w| depth.get(w).copied()).min() else {
            continue;
        };
        if let Some(d) = depth.get_mut(v)
            && least > *d + 1
        {
            *d = least - 1;
        }
    }
}

/// The connected parts of the calls among `nodes`, each in display order, the largest first
/// (ties by where they start in display order).
pub(crate) fn parts(f: &Flow<'_>, nodes: &[usize]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..f.ins.len()).collect();
    fn root(parent: &mut [usize], mut v: usize) -> usize {
        while let Some(&p) = parent.get(v) {
            if p == v {
                break;
            }
            let gp = parent.get(p).copied().unwrap_or(p);
            if let Some(x) = parent.get_mut(v) {
                *x = gp;
            }
            v = gp;
        }
        v
    }
    for &u in nodes {
        for &w in f.calls(u) {
            let (a, b) = (root(&mut parent, u), root(&mut parent, w));
            if a != b
                && let Some(x) = parent.get_mut(a.max(b))
            {
                *x = a.min(b);
            }
        }
    }
    let mut sorted = nodes.to_vec();
    sorted.sort_unstable();
    let mut by_root: HashMap<usize, usize> = HashMap::new();
    let mut out: Vec<Vec<usize>> = Vec::new();
    for v in sorted {
        let r = root(&mut parent, v);
        let k = *by_root.entry(r).or_insert_with(|| {
            out.push(Vec::new());
            out.len() - 1
        });
        if let Some(p) = out.get_mut(k) {
            p.push(v);
        }
    }
    out.sort_by(|a, b| {
        b.len()
            .cmp(&a.len())
            .then_with(|| a.first().cmp(&b.first()))
    });
    out
}
