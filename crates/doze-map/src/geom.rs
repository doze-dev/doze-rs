//! Points, rectangles and the drawable line: a path of absolute moves, lines, quadratic and cubic
//! curves, the four commands the Go map wrote as SVG (`M`, `L`, `Q`, `C`).

use std::fmt::Write as _;

/// A point in the map's pixels (CSS pixels at zoom 1).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pt {
    /// Across, growing to the right.
    pub x: f64,
    /// Down, growing downwards.
    pub y: f64,
}

impl Pt {
    /// The point at `x`, `y`.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub(crate) fn dist(self, o: Pt) -> f64 {
        (o.x - self.x).hypot(o.y - self.y)
    }

    /// The point `r` along the way from `self` to `to`.
    pub(crate) fn toward(self, to: Pt, r: f64) -> Pt {
        let d = self.dist(to);
        if d == 0.0 {
            return self;
        }
        Pt::new(
            self.x + (to.x - self.x) * r / d,
            self.y + (to.y - self.y) * r / d,
        )
    }

    /// The direction from `self` to `b`, of length one; zero when they meet.
    pub(crate) fn unit(self, b: Pt) -> Pt {
        let l = self.dist(b);
        if l == 0.0 {
            return Pt::default();
        }
        Pt::new((b.x - self.x) / l, (b.y - self.y) / l)
    }
}

/// An axis-aligned rectangle: a card, a caption, the drawing's bounds, a viewport.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub w: f64,
    /// Height.
    pub h: f64,
}

impl Rect {
    /// The rectangle at `x`, `y` of size `w` × `h`.
    #[must_use]
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
    /// Right edge.
    #[must_use]
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    /// Bottom edge.
    #[must_use]
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    /// Centre.
    #[must_use]
    pub fn center(&self) -> Pt {
        Pt::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    /// Whether the two overlap (touching edges do not count).
    #[must_use]
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
    /// Whether `p` is inside, short of a margin of two pixels: a line that runs along a card's
    /// edge, or starts on it, is not behind it.
    #[must_use]
    pub fn inside(&self, p: Pt) -> bool {
        const M: f64 = 2.0;
        p.x > self.x + M && p.x < self.right() - M && p.y > self.y + M && p.y < self.bottom() - M
    }
    /// The smallest rectangle holding both.
    #[must_use]
    pub fn union(&self, o: &Rect) -> Rect {
        let (x, y) = (self.x.min(o.x), self.y.min(o.y));
        Rect::new(
            x,
            y,
            self.right().max(o.right()) - x,
            self.bottom().max(o.bottom()) - y,
        )
    }
    pub(crate) fn translate(&self, dx: f64, dy: f64) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }
}

/// One command of a path, in absolute coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Seg {
    /// Start here.
    Move(Pt),
    /// A straight line to the point.
    Line(Pt),
    /// A quadratic curve: control point, end.
    Quad(Pt, Pt),
    /// A cubic curve: two control points, end.
    Cubic(Pt, Pt, Pt),
}

impl Seg {
    /// Where the command ends.
    #[must_use]
    pub fn end(&self) -> Pt {
        match *self {
            Seg::Move(p) | Seg::Line(p) | Seg::Quad(_, p) | Seg::Cubic(_, _, p) => p,
        }
    }
    fn points(&self) -> impl Iterator<Item = Pt> {
        let (a, b, c) = match *self {
            Seg::Move(p) | Seg::Line(p) => (p, p, p),
            Seg::Quad(c, p) => (c, p, p),
            Seg::Cubic(c1, c2, p) => (c1, c2, p),
        };
        [a, b, c].into_iter()
    }
    pub(crate) fn translate(&self, dx: f64, dy: f64) -> Seg {
        let t = |p: Pt| Pt::new(p.x + dx, p.y + dy);
        match *self {
            Seg::Move(p) => Seg::Move(t(p)),
            Seg::Line(p) => Seg::Line(t(p)),
            Seg::Quad(c, p) => Seg::Quad(t(c), t(p)),
            Seg::Cubic(a, b, p) => Seg::Cubic(t(a), t(b), t(p)),
        }
    }
}

/// How a line ends: a call in an arrowhead the renderer draws past the path's end, a use in a dot
/// on the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum End {
    /// An arrowhead, `HEAD_LEN` long, its base at the path's end.
    Arrow,
    /// A dot at the path's end.
    Dot,
}

/// A drawable line.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    /// The commands, the first a [`Seg::Move`].
    pub segs: Vec<Seg>,
    /// What the renderer draws at the end.
    pub end: End,
}

impl Path {
    /// The SVG path data, written as the Go map wrote it (`M24 229 C …`).
    #[must_use]
    pub fn d(&self) -> String {
        let mut s = String::new();
        for seg in &self.segs {
            let _ = match *seg {
                Seg::Move(p) => write!(s, "M{} {}", n(p.x), n(p.y)),
                Seg::Line(p) => write!(s, " L {} {}", n(p.x), n(p.y)),
                Seg::Quad(c, p) => write!(s, " Q {} {}, {} {}", n(c.x), n(c.y), n(p.x), n(p.y)),
                Seg::Cubic(a, b, p) => write!(
                    s,
                    " C {} {}, {} {}, {} {}",
                    n(a.x),
                    n(a.y),
                    n(b.x),
                    n(b.y),
                    n(p.x),
                    n(p.y)
                ),
            };
        }
        s
    }

    /// A rectangle holding the whole line (its control points bound its curves).
    #[must_use]
    pub fn bbox(&self) -> Rect {
        let mut it = self.segs.iter().flat_map(Seg::points);
        let Some(first) = it.next() else {
            return Rect::default();
        };
        let (mut lo, mut hi) = (first, first);
        for p in it {
            lo = Pt::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Pt::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        Rect::new(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y)
    }

    /// The ends of its commands, in order: the line drawn straight between them is the cheap
    /// version a renderer shows when zoomed far out.
    #[must_use]
    pub fn waypoints(&self) -> Vec<Pt> {
        self.segs.iter().map(Seg::end).collect()
    }

    /// Where the line ends and which way it is heading there, for the arrowhead.
    #[must_use]
    pub fn tip(&self) -> Option<(Pt, Pt)> {
        let mut cur = Pt::default();
        let mut dir = Pt::new(1.0, 0.0);
        for seg in &self.segs {
            let prev = cur;
            cur = seg.end();
            let from = match *seg {
                Seg::Move(_) => continue,
                Seg::Line(_) => prev,
                Seg::Quad(c, _) => c,
                Seg::Cubic(_, c2, _) if c2 == cur => prev,
                Seg::Cubic(_, c2, _) => c2,
            };
            dir = Pt::new(cur.x - from.x, cur.y - from.y);
        }
        let len = dir.x.hypot(dir.y);
        (len > 0.0).then(|| (cur, Pt::new(dir.x / len, dir.y / len)))
    }

    pub(crate) fn translate(&self, dx: f64, dy: f64) -> Path {
        Path {
            segs: self.segs.iter().map(|s| s.translate(dx, dy)).collect(),
            end: self.end,
        }
    }
}

/// A coordinate without a needless fraction, as the Go map wrote it.
fn n(f: f64) -> String {
    if f == f.trunc() && f.abs() < 1e15 {
        // Integral and in range, so the cast is exact.
        #[allow(clippy::cast_possible_truncation)]
        let i = f as i64;
        i.to_string()
    } else {
        format!("{f:.1}")
    }
}
