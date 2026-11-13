//! Routes a bundle between two named regions through the gaps between frames, so it passes over no other region's
//! cards: out of the side of one frame that faces the other, along the gutters the packing leaves (a shortest path on
//! a coarse grid that every frame blocks, turns costing extra so the line keeps to few corners), and into the facing
//! side of the other, corners rounded. A map too large for the grid (thousands of services, far past the Map's target)
//! keeps the plain curve.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::draw::{HEAD_LEN, elbow_h};
use crate::geom::{Path, Pt, Rect};

/// The grid's cells at most, and the bundles routed at most: past them a bundle is a plain curve.
const MAX_CELLS: usize = 160_000;
/// A frame's keep-out margin, so a line runs down the middle of a gutter rather than along a frame.
const MARGIN: f64 = 10.0;
/// The height of a frame's name above its top edge.
const PILL: f64 = 12.0;
/// What sharing a cell with a bundle routed before costs, in steps, for each (up to three).
const SHARE: u32 = 3;
/// What a turn costs, in steps.
const TURN: u32 = 6;

/// The grid over the frames, built once for every bundle of a map.
pub(crate) struct Grid {
    x0: f64,
    y0: f64,
    cell: f64,
    cols: usize,
    rows: usize,
    blocked: Vec<bool>,
    /// How many routed bundles pass each cell: the next one pays to share it, so bundles spread over the gaps
    /// rather than merging into one trunk where there is room.
    used: Vec<u8>,
}

impl Grid {
    pub(crate) fn new(frames: &[Rect]) -> Option<Grid> {
        let first = frames.first()?;
        let all = frames.iter().skip(1).fold(*first, |a, r| a.union(r));
        let cell = (all.w.max(all.h) / 300.0).clamp(12.0, 64.0);
        let pad = cell * 3.0;
        let (x0, y0) = (all.x - pad, all.y - pad);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (cols, rows) = (
            ((all.w + 2.0 * pad) / cell).ceil() as usize,
            ((all.h + 2.0 * pad) / cell).ceil() as usize,
        );
        if cols * rows > MAX_CELLS {
            return None;
        }
        let mut g = Grid {
            x0,
            y0,
            cell,
            cols,
            rows,
            blocked: vec![false; cols * rows],
            used: vec![0; cols * rows],
        };
        for f in frames {
            // The frame's name sits on its top edge, half above it: the margin there is wider.
            let r = Rect::new(
                f.x - MARGIN,
                f.y - MARGIN - PILL,
                f.w + 2.0 * MARGIN,
                f.h + 2.0 * MARGIN + PILL,
            );
            let (c0, r0) = g.at(Pt::new(r.x, r.y));
            let (c1, r1) = g.at(Pt::new(r.right(), r.bottom()));
            for row in r0..=r1 {
                for col in c0..=c1 {
                    if r.inside(g.centre(col, row))
                        && let Some(b) = g.blocked.get_mut(row * cols + col)
                    {
                        *b = true;
                    }
                }
            }
        }
        Some(g)
    }

    /// The cell holding `p`, clamped to the grid.
    fn at(&self, p: Pt) -> (usize, usize) {
        let f = |v: f64, n: usize| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let i = (v / self.cell).floor().clamp(0.0, (n - 1) as f64) as usize;
            i
        };
        (f(p.x - self.x0, self.cols), f(p.y - self.y0, self.rows))
    }

    fn centre(&self, col: usize, row: usize) -> Pt {
        Pt::new(
            self.x0 + (col as f64 + 0.5) * self.cell,
            self.y0 + (row as f64 + 0.5) * self.cell,
        )
    }

    fn free(&self, col: usize, row: usize) -> bool {
        !self
            .blocked
            .get(row * self.cols + col)
            .copied()
            .unwrap_or(true)
    }

    /// A shortest path of free cells, turns costing [`TURN`] steps (A*, 4 ways, the heading in the state).
    fn path(&self, from: (usize, usize), to: (usize, usize)) -> Option<Vec<(usize, usize)>> {
        const DIRS: [(i64, i64); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
        let n = self.cols * self.rows;
        let idx = |c: usize, r: usize, d: usize| (r * self.cols + c) * 4 + d;
        let h = |c: usize, r: usize| (c.abs_diff(to.0) + r.abs_diff(to.1)) as u32;
        let mut best = vec![u32::MAX; n * 4];
        let mut prev = vec![usize::MAX; n * 4];
        let mut heap = BinaryHeap::new();
        for d in 0..4 {
            let i = idx(from.0, from.1, d);
            if let Some(b) = best.get_mut(i) {
                *b = 0;
            }
            heap.push(Reverse((h(from.0, from.1), 0u32, i)));
        }
        while let Some(Reverse((_, g, i))) = heap.pop() {
            if best.get(i).is_some_and(|&b| g > b) {
                continue;
            }
            let (cell, d) = (i / 4, i % 4);
            let (c, r) = (cell % self.cols, cell / self.cols);
            if (c, r) == to {
                return Some(self.back(&prev, i));
            }
            for (nd, (dx, dy)) in DIRS.iter().enumerate() {
                let (Some(nc), Some(nr)) = (
                    c.checked_add_signed(*dx as isize),
                    r.checked_add_signed(*dy as isize),
                ) else {
                    continue;
                };
                if nc >= self.cols || nr >= self.rows || !(self.free(nc, nr) || (nc, nr) == to) {
                    continue;
                }
                let shared = u32::from(self.used.get(nr * self.cols + nc).copied().unwrap_or(0));
                let ng = g + 1 + SHARE * shared.min(3) + if nd == d { 0 } else { TURN };
                let j = idx(nc, nr, nd);
                if best.get(j).is_some_and(|&b| ng < b) {
                    if let Some(b) = best.get_mut(j) {
                        *b = ng;
                    }
                    if let Some(p) = prev.get_mut(j) {
                        *p = i;
                    }
                    heap.push(Reverse((ng + h(nc, nr), ng, j)));
                }
            }
        }
        None
    }

    fn back(&self, prev: &[usize], mut i: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        loop {
            let cell = i / 4;
            let at = (cell % self.cols, cell / self.cols);
            if out.last() != Some(&at) {
                out.push(at);
            }
            match prev.get(i) {
                Some(&p) if p != usize::MAX => i = p,
                _ => break,
            }
        }
        out.reverse();
        out
    }
}

/// Which side of `a` faces `b`, as the unit normal out of it, and the point on it a line leaves from.
fn side(a: &Rect, b: &Rect) -> (Pt, Pt) {
    let mid = |lo: f64, hi: f64, olo: f64, ohi: f64| {
        let (l, h) = (lo.max(olo), hi.min(ohi));
        if l < h {
            f64::midpoint(l, h)
        } else {
            f64::midpoint(lo, hi)
        }
    };
    if b.x >= a.right() {
        (
            Pt::new(1.0, 0.0),
            Pt::new(a.right(), mid(a.y, a.bottom(), b.y, b.bottom())),
        )
    } else if b.right() <= a.x {
        (
            Pt::new(-1.0, 0.0),
            Pt::new(a.x, mid(a.y, a.bottom(), b.y, b.bottom())),
        )
    } else if b.y >= a.bottom() {
        (
            Pt::new(0.0, 1.0),
            Pt::new(mid(a.x, a.right(), b.x, b.right()), a.bottom()),
        )
    } else {
        (
            Pt::new(0.0, -1.0),
            Pt::new(mid(a.x, a.right(), b.x, b.right()), a.y),
        )
    }
}

/// The bundle from frame `a` to frame `b` (indexes into `frames`) through the gaps, or `None` when the grid is too
/// large or no way through is found.
pub(crate) fn route(g: &mut Grid, frames: &[Rect], a: usize, b: usize) -> Option<Path> {
    let (ra, rb) = (frames.get(a)?, frames.get(b)?);
    let (na, sa) = side(ra, rb);
    let (nb, sb) = side(rb, ra);
    let out = MARGIN + g.cell;
    let from = g.at(Pt::new(sa.x + na.x * out, sa.y + na.y * out));
    let to = g.at(Pt::new(sb.x + nb.x * out, sb.y + nb.y * out));
    let cells = g.path(from, to)?;
    for &(c, r) in &cells {
        if let Some(u) = g.used.get_mut(r * g.cols + c) {
            *u = u.saturating_add(1);
        }
    }
    let mut pts: Vec<Pt> = cells.iter().map(|&(c, r)| g.centre(c, r)).collect();
    // Leave and land square: the end points slide along their sides to the path's first and last cells.
    let first = *pts.first()?;
    let last = *pts.last()?;
    let slide = |s: Pt, n: Pt, p: Pt, r: &Rect| {
        if n.x != 0.0 {
            Pt::new(s.x, p.y.clamp(r.y + 12.0, r.bottom() - 12.0))
        } else {
            Pt::new(p.x.clamp(r.x + 12.0, r.right() - 12.0), s.y)
        }
    };
    let (s, e) = (slide(sa, na, first, ra), slide(sb, nb, last, rb));
    let bend = |s: Pt, n: Pt, p: Pt| {
        if n.x != 0.0 {
            Pt::new(p.x, s.y)
        } else {
            Pt::new(s.x, p.y)
        }
    };
    pts.insert(0, bend(s, na, first));
    pts.insert(0, s);
    pts.push(bend(e, nb, last));
    pts.push(e);
    Some(elbow_h(HEAD_LEN, &simplify(&pts)).path)
}

/// Drops the points that sit on a straight run.
fn simplify(pts: &[Pt]) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::with_capacity(pts.len());
    for &p in pts {
        if out.last().is_some_and(|q| q.dist(p) < 0.5) {
            continue;
        }
        if let [.., a, b] = out.as_slice() {
            let straight = (a.x == b.x && b.x == p.x) || (a.y == b.y && b.y == p.y);
            if straight {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::route;
    use crate::geom::{Pt, Rect, Seg};

    /// Points along a path's straight runs and the ends of its curves.
    fn along(p: &crate::geom::Path) -> Vec<Pt> {
        let mut out = Vec::new();
        let mut cur = Pt::default();
        for s in &p.segs {
            let e = s.end();
            if let Seg::Line(_) = s {
                for i in 0..=20 {
                    let t = f64::from(i) / 20.0;
                    out.push(Pt::new(
                        cur.x + (e.x - cur.x) * t,
                        cur.y + (e.y - cur.y) * t,
                    ));
                }
            }
            out.push(e);
            cur = e;
        }
        out
    }

    /// Two frames with a third between them: the line goes round it, never over it.
    #[test]
    fn a_bundle_goes_round_the_frames_between() {
        let frames = [
            Rect::new(0.0, 0.0, 400.0, 300.0),
            Rect::new(500.0, 0.0, 400.0, 300.0),
            Rect::new(1000.0, 0.0, 400.0, 300.0),
            Rect::new(0.0, 360.0, 1400.0, 300.0),
        ];
        let mut g = super::Grid::new(&frames).unwrap();
        let p = route(&mut g, &frames, 0, 2).expect("a way round");
        for q in along(&p) {
            for (i, f) in frames.iter().enumerate() {
                if i == 1 || i == 3 {
                    assert!(!f.inside(q), "passes over frame {i} at {q:?}");
                }
            }
        }
        // It leaves frame 0 on its right and lands on frame 2's left.
        assert!(matches!(p.segs.first(), Some(Seg::Move(s)) if (s.x - 400.0).abs() < 1e-6));
        let tip = p.tip().unwrap().0;
        assert!((tip.x - 1000.0).abs() < 12.0, "{tip:?}");
    }
}
