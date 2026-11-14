//! The calls' lines, by construction rather than by search: out of the caller's side at a port
//! of its own, across each gap between columns in one S-curve, straight through each column it
//! crosses at its placeholder's reserved height, and into the called card's side, flat, where the
//! arrowhead lands. No line can pass behind a card: the gaps hold none, and a placeholder's
//! height is kept clear in its column.

use crate::draw::{HEAD_LEN, curve_h};
use crate::geom::{End, Path, Pt, Rect, Seg};
use crate::layered::rank::Lg;

/// A part's geometry: its columns and each vertex's centre y.
#[derive(Debug, Clone)]
pub(crate) struct Geo {
    pub(crate) col_x: Vec<f64>,
    pub(crate) col_w: Vec<f64>,
    pub(crate) y: Vec<f64>,
}

impl Geo {
    fn col(&self, l: usize) -> (f64, f64) {
        (
            self.col_x.get(l).copied().unwrap_or(0.0),
            self.col_w.get(l).copied().unwrap_or(0.0),
        )
    }
    pub(crate) fn rect(&self, lg: &Lg, v: usize, h: f64) -> Rect {
        let (x, w) = self.col(lg.layer.get(v).copied().unwrap_or(0));
        let cy = self.y.get(v).copied().unwrap_or(0.0);
        Rect::new(x, cy - h / 2.0, w, h)
    }
}

/// One routed call: caller and called instance, the line, and whether it runs back.
pub(crate) type Routed = (usize, usize, Path, bool);

/// Ports: the height each chain leaves or enters a card at, on the right (`out`) or left side.
fn ports(lg: &Lg, geo: &Geo, h: f64) -> (Vec<f64>, Vec<f64>) {
    let n = lg.chains.len();
    let (mut right, mut left) = (vec![0.0; n], vec![0.0; n]);
    let mut sides: Vec<(Vec<Port>, Vec<Port>)> = vec![(Vec::new(), Vec::new()); lg.len()];
    let ny = |v: Option<&usize>| {
        v.map_or((0.0, usize::MAX), |&v| {
            (geo.y.get(v).copied().unwrap_or(0.0), v)
        })
    };
    for (c, ch) in lg.chains.iter().enumerate() {
        let (Some(&first), Some(&last)) = (ch.verts.first(), ch.verts.last()) else {
            continue;
        };
        if let Some(s) = sides.get_mut(first) {
            let (y, next) = ny(ch.verts.get(1));
            s.0.push(Port { y, next, chain: c });
        }
        if let Some(s) = sides.get_mut(last) {
            let (y, next) = ny(ch.verts.len().checked_sub(2).and_then(|i| ch.verts.get(i)));
            s.1.push(Port { y, next, chain: c });
        }
    }
    for (v, (r, l)) in sides.iter_mut().enumerate() {
        let top = geo.y.get(v).copied().unwrap_or(0.0) - h / 2.0;
        spread(r, top, h, &mut right);
        spread(l, top, h, &mut left);
    }
    (right, left)
}

/// A chain's end on a card's side: where its next vertex is, which, and the chain.
#[derive(Debug, Clone, Copy)]
struct Port {
    y: f64,
    next: usize,
    chain: usize,
}

/// The chains on one side of a card, spread down it in the order of where they go; chains into
/// the same trunk share a port.
fn spread(side: &mut [Port], top: f64, h: f64, out: &mut [f64]) {
    side.sort_by(|a, b| {
        a.y.total_cmp(&b.y)
            .then(a.next.cmp(&b.next))
            .then(a.chain.cmp(&b.chain))
    });
    let mut groups: Vec<usize> = side.iter().map(|p| p.next).collect();
    groups.dedup();
    let n = groups.len() as f64;
    let inner = h - 24.0;
    let mut k = 0usize;
    for (i, p) in side.iter().enumerate() {
        if i > 0 && side.get(i - 1).is_some_and(|q| q.next != p.next) {
            k += 1;
        }
        let y = if groups.len() == 1 {
            top + h / 2.0
        } else {
            top + 12.0 + inner * (k as f64 + 1.0) / (n + 1.0)
        };
        if let Some(o) = out.get_mut(p.chain) {
            *o = y.round();
        }
    }
}

/// Every call's line in a part.
pub(crate) fn route(lg: &Lg, geo: &Geo, h: f64) -> Vec<Routed> {
    let (right, left) = ports(lg, geo, h);
    lg.chains
        .iter()
        .enumerate()
        .filter_map(|(c, ch)| {
            let (&first, &last) = (ch.verts.first()?, ch.verts.last()?);
            let start = Pt::new(geo.rect(lg, first, h).right(), right.get(c).copied()?);
            let end = Pt::new(geo.rect(lg, last, h).x, left.get(c).copied()?);
            let mut pts = vec![start];
            for &d in ch
                .verts
                .get(1..ch.verts.len().saturating_sub(1))
                .unwrap_or(&[])
            {
                let (x, w) = geo.col(lg.layer.get(d).copied().unwrap_or(0));
                let y = geo.y.get(d).copied().unwrap_or(0.0);
                pts.extend([Pt::new(x, y), Pt::new(x + w, y)]);
            }
            pts.push(end);
            if ch.back {
                pts.reverse();
            }
            Some((ch.from, ch.to, through(&pts), ch.back))
        })
        .collect()
}

/// The line through `pts`: hops across gaps (even steps) and runs through columns (odd steps),
/// the last hop landing flat with room for the head.
fn through(pts: &[Pt]) -> Path {
    let Some(&first) = pts.first() else {
        return Path {
            segs: Vec::new(),
            end: End::Arrow,
        };
    };
    let mut segs = vec![Seg::Move(first)];
    let hops = pts.len().saturating_sub(1);
    for (i, w) in pts.windows(2).enumerate() {
        let [a, b] = [
            w.first().copied().unwrap_or(first),
            w.last().copied().unwrap_or(first),
        ];
        let m = f64::midpoint(a.x, b.x);
        if i + 1 == hops {
            let landed = curve_h(HEAD_LEN, a, Pt::new(m, a.y), Pt::new(m, b.y), b);
            segs.extend(landed.path.segs.into_iter().skip(1));
        } else if i % 2 == 1 || a.y == b.y {
            segs.push(Seg::Line(b));
        } else {
            segs.push(Seg::Cubic(Pt::new(m, a.y), Pt::new(m, b.y), b));
        }
    }
    Path {
        segs,
        end: End::Arrow,
    }
}
