//! The line shapes: a cubic that lands flat on a card's side, and a polyline whose corners are
//! rounded. Each comes with samples along it, so a router can check it passes behind no card
//! before taking it (geometry.go in the Go map, which these reproduce point for point).

use crate::geom::{End, Path, Pt, Rect, Seg};

/// The arrowhead's length: the line stops where the head begins, and the renderer draws the head
/// from there, `HEAD_LEN` further on, so no line shows through it.
pub const HEAD_LEN: f64 = 10.0;
/// The straight run before the head, so the head sits on the line's end and not on a bend.
pub(crate) const STUB_LEN: f64 = 8.0;

/// A path and the samples a router checks it by.
#[derive(Debug, Clone)]
pub(crate) struct Drawn {
    pub(crate) path: Path,
    pub(crate) pts: Vec<Pt>,
}

impl Drawn {
    /// No sample is behind a card other than one of `ends`.
    pub(crate) fn clear(&self, cards: &[Rect], ends: &[Rect]) -> bool {
        self.pts
            .iter()
            .all(|&p| cards.iter().all(|c| ends.contains(c) || !c.inside(p)))
    }
}

fn end_of(head: f64) -> End {
    if head > 0.0 { End::Arrow } else { End::Dot }
}

/// One cubic from `a` toward `b`, landing on `b` with a head of length `head` (0 for a dot).
pub(crate) fn curve_h(head: f64, a: Pt, c1: Pt, c2: Pt, b: Pt) -> Drawn {
    let mut d = c2.unit(b);
    if d == Pt::default() {
        d = a.unit(b);
    }
    let e = Pt::new(b.x - head * d.x, b.y - head * d.y);
    let st = Pt::new(e.x - STUB_LEN * d.x, e.y - STUB_LEN * d.y);
    let k = head + STUB_LEN;
    let c2s = Pt::new(c2.x - k * d.x, c2.y - k * d.y);
    let end = end_of(head);
    if a.dist(b) <= k {
        // Too short to bend: straight.
        let path = Path {
            segs: vec![Seg::Move(a), Seg::Line(e)],
            end,
        };
        return Drawn {
            path,
            pts: line(a, b),
        };
    }
    let path = Path {
        segs: vec![Seg::Move(a), Seg::Cubic(c1, c2s, st), Seg::Line(e)],
        end,
    };
    let mut pts = cubic_samples(a, c1, c2s, st);
    pts.extend(line(st, b));
    Drawn { path, pts }
}

/// [`curve_h`] with an arrowhead.
pub(crate) fn curve(a: Pt, c1: Pt, c2: Pt, b: Pt) -> Drawn {
    curve_h(HEAD_LEN, a, c1, c2, b)
}

/// A polyline through the points, corners rounded, ending in a head of length `head`.
pub(crate) fn elbow_h(head: f64, points: &[Pt]) -> Drawn {
    let points = dedupe(points);
    let end = end_of(head);
    let Some((&first, rest)) = points.split_first() else {
        return Drawn {
            path: Path {
                segs: Vec::new(),
                end,
            },
            pts: Vec::new(),
        };
    };
    let mut segs = vec![Seg::Move(first)];
    let mut pts = Vec::new();
    let mut cur = first;
    for (i, &p) in rest.iter().enumerate() {
        let Some(&next) = rest.get(i + 1) else {
            let e = p.toward(cur, head.min(cur.dist(p)));
            segs.push(Seg::Line(e));
            pts.extend(line(cur, p));
            break;
        };
        // Round the corner at p: stop short of it, curve through it.
        let r = 18f64.min(cur.dist(p).min(p.dist(next)) / 2.0);
        let (inp, out) = (p.toward(cur, r), p.toward(next, r));
        segs.push(Seg::Line(inp));
        segs.push(Seg::Quad(p, out));
        pts.extend(line(cur, inp));
        pts.extend(line(inp, out));
        cur = out;
    }
    Drawn {
        path: Path { segs, end },
        pts,
    }
}

/// [`elbow_h`] with an arrowhead.
pub(crate) fn elbow(points: &[Pt]) -> Drawn {
    elbow_h(HEAD_LEN, points)
}

/// The points without repeats, and without a point on the straight line between its neighbours.
fn dedupe(points: &[Pt]) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::with_capacity(points.len());
    for &p in points {
        match out.as_slice() {
            [.., last] if *last == p => {}
            [.., a, b] if (a.x == b.x && b.x == p.x) || (a.y == b.y && b.y == p.y) => {
                if let Some(l) = out.last_mut() {
                    *l = p;
                }
            }
            _ => out.push(p),
        }
    }
    out
}

/// Samples along a straight line, about every six pixels.
pub(crate) fn line(a: Pt, b: Pt) -> Vec<Pt> {
    // Non-negative and bounded by the drawing's size, so the cast is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let steps = (a.dist(b) / 6.0) as usize + 1;
    (0..=steps)
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let t = i as f64 / steps as f64;
            Pt::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
        })
        .collect()
}

/// 49 samples along a cubic.
pub(crate) fn cubic_samples(a: Pt, c1: Pt, c2: Pt, b: Pt) -> Vec<Pt> {
    const STEPS: u32 = 48;
    (0..=STEPS)
        .map(|i| {
            let t = f64::from(i) / f64::from(STEPS);
            let u = 1.0 - t;
            let (k0, k1, k2, k3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            Pt::new(
                k0 * a.x + k1 * c1.x + k2 * c2.x + k3 * b.x,
                k0 * a.y + k1 * c1.y + k2 * c2.y + k3 * b.y,
            )
        })
        .collect()
}

/// A plain curve from card `a` to card `b`, side to side, for lines no router places (calls between
/// named regions, a focus view's calls): from `a`'s right into `b`'s left when `b` is to the
/// right, from `a`'s left into `b`'s right when it is to the left, and out of `a`'s right and back
/// into `b`'s right when the two share a column.
pub(crate) fn link(a: &Rect, b: &Rect) -> Path {
    let (ca, cb) = (a.center(), b.center());
    let (start, end, out, back) = if b.x >= a.right() {
        (Pt::new(a.right(), ca.y), Pt::new(b.x, cb.y), 1.0, -1.0)
    } else if b.right() <= a.x {
        (Pt::new(a.x, ca.y), Pt::new(b.right(), cb.y), -1.0, 1.0)
    } else {
        (Pt::new(a.right(), ca.y), Pt::new(b.right(), cb.y), 1.0, 1.0)
    };
    let pull = ((end.x - start.x).abs() / 2.0)
        .max((end.y - start.y).abs() / 4.0)
        .max(48.0);
    curve(
        start,
        Pt::new(start.x + out * pull, start.y),
        Pt::new(end.x + back * pull, end.y),
        end,
    )
    .path
}
