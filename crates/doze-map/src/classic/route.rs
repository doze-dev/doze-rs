//! The Go map's router: the board's line shapes first, each taken only if it passes behind no
//! card, then along the free rows and columns between the cards. Every card is an obstacle to
//! every line but its own two. Fine for a few dozen cards; the layered map routes by construction.

use std::collections::HashSet;

use crate::classic::Bx;
use crate::draw::{Drawn, curve, curve_h, elbow, elbow_h};
use crate::geom::{Pt, Rect};

pub(crate) struct Router {
    cards: Vec<Rect>,
    /// The free columns and rows between cards.
    xs: Vec<f64>,
    ys: Vec<f64>,
    /// The sides of a card a line already left from, so a second line takes the other side.
    used: HashSet<(usize, bool)>,
}

impl Router {
    pub(crate) fn new(cards: &[Bx]) -> Self {
        Router {
            cards: cards.iter().map(Bx::rect).collect(),
            xs: gaps(cards.iter().map(|b| (b.x, b.right()))),
            ys: gaps(cards.iter().map(|b| (b.y, b.bottom()))),
            used: HashSet::new(),
        }
    }

    fn take(&self, p: Drawn, s: &Bx, t: &Bx) -> Option<Drawn> {
        p.clear(&self.cards, &[s.rect(), t.rect()]).then_some(p)
    }

    /// A call's line from the caller's card `s` (its index `from`) to the called card `t`.
    pub(crate) fn call(&mut self, from: usize, s: &Bx, t: &Bx) -> Drawn {
        let (sx, sy) = (s.right() as f64, s.cy() as f64);
        let (tx, ty) = (t.x as f64, t.cy() as f64);
        if t.x > s.right() {
            // Side to side, when nothing stands between: straight across on one row; to a card
            // above or below, out a short way, a rounded bend, and in level.
            if sy == ty {
                let m = f64::midpoint(sx, tx);
                let p = curve(
                    Pt::new(sx, sy),
                    Pt::new(m, sy),
                    Pt::new(m, ty),
                    Pt::new(tx, ty),
                );
                if let Some(p) = self.take(p, s, t) {
                    return p;
                }
            }
            let bx = sx + 24f64.min((tx - sx) / 3.0);
            let pts = [
                Pt::new(sx, sy),
                Pt::new(bx, sy),
                Pt::new(bx, ty),
                Pt::new(tx, ty),
            ];
            if let Some(p) = self.take(elbow(&pts), s, t) {
                return p;
            }
            if let Some(p) = self.around(from, s, t) {
                return p;
            }
        }
        if let Some(p) = self.along_gaps(s, t) {
            return p;
        }
        let mx = f64::midpoint(sx, tx);
        curve(
            Pt::new(sx, sy),
            Pt::new(mx, sy),
            Pt::new(mx, ty),
            Pt::new(tx, ty),
        )
    }

    /// Over the cards between, along its own row, or under them.
    fn around(&mut self, from: usize, s: &Bx, t: &Bx) -> Option<Drawn> {
        if !self.used.contains(&(from, true))
            && let Some(p) = self.take(self.over(s, t), s, t)
        {
            self.used.insert((from, true));
            return Some(p);
        }
        if s.cy() != t.cy()
            && let Some(p) = self.take(along(s, t), s, t)
        {
            return Some(p);
        }
        if !self.used.contains(&(from, false))
            && let Some(p) = self.take(self.under(s, t), s, t)
        {
            self.used.insert((from, false));
            return Some(p);
        }
        self.take(self.over(s, t), s, t)
            .or_else(|| self.take(self.under(s, t), s, t))
    }

    /// Up out of the caller's top into the free row above, along it, and down into the called.
    fn over(&self, s: &Bx, t: &Bx) -> Drawn {
        let (x1, x2) = ((s.right() - 28) as f64, (t.x + 28) as f64);
        let cy = self.row_above(s.y.min(t.y) as f64);
        elbow(&[
            Pt::new(x1, s.y as f64),
            Pt::new(x1, cy),
            Pt::new(x2, cy),
            Pt::new(x2, t.y as f64),
        ])
    }

    fn under(&self, s: &Bx, t: &Bx) -> Drawn {
        let (x1, x2) = ((s.right() - 28) as f64, (t.x + 28) as f64);
        let (sb, tb) = (s.bottom() as f64, t.bottom() as f64);
        let cy = self.row_below(sb.max(tb));
        elbow(&[
            Pt::new(x1, sb),
            Pt::new(x1, cy),
            Pt::new(x2, cy),
            Pt::new(x2, tb),
        ])
    }

    /// Out of the source's right side (left, for a line going back), along the free rows and
    /// columns, into the target's side.
    fn along_gaps(&self, s: &Bx, t: &Bx) -> Option<Drawn> {
        let (sy, ty) = (s.cy() as f64, t.cy() as f64);
        let (sx, gx1) = (s.right() as f64, self.next_x(s.right() as f64, 1.0));
        let (tx, gx2) = if t.x <= s.x {
            (t.right() as f64, self.next_x(t.right() as f64, 1.0))
        } else {
            (t.x as f64, self.next_x(t.x as f64, -1.0))
        };
        let mut rows = vec![sy, ty];
        rows.extend(self.ys.iter().copied());
        rows.sort_by(|a, b| {
            ((a - sy).abs() + (a - ty).abs()).total_cmp(&((b - sy).abs() + (b - ty).abs()))
        });
        rows.into_iter().find_map(|cy| {
            let pts = [
                Pt::new(sx, sy),
                Pt::new(gx1, sy),
                Pt::new(gx1, cy),
                Pt::new(gx2, cy),
                Pt::new(gx2, ty),
                Pt::new(tx, ty),
            ];
            self.take(elbow(&pts), s, t)
        })
    }

    /// The dashed line from an app's card down to a band card: the board's S from the bottom of
    /// one to the top of the other when that is clear, else down the gap beside the card and
    /// along the row above the band card. It ends in a dot, not a head.
    pub(crate) fn use_line(&self, s: &Bx, t: &Bx) -> Drawn {
        let mut a = Pt::new(s.cx() as f64, s.bottom() as f64);
        let mut b = Pt::new(t.cx() as f64, t.y as f64);
        if t.y < s.y {
            (a.y, b.y) = (s.y as f64, t.bottom() as f64);
        }
        let my = f64::midpoint(a.y, b.y);
        let s_curve = || curve_h(0.0, a, Pt::new(a.x, my), Pt::new(b.x, my), b);
        if let Some(p) = self.take(s_curve(), s, t) {
            return p;
        }
        let above = self.row_above(t.y as f64);
        let pts = [a, Pt::new(a.x, above), Pt::new(b.x, above), b];
        if let Some(p) = self.take(elbow_h(0.0, &pts), s, t) {
            return p;
        }
        // Down the gap beside the card: the left first, since calls leave from the right; a
        // little below the middle, clear of a call arriving there.
        let y0 = (s.cy() + 18) as f64;
        for (side, x0) in [(-1.0, s.x as f64), (1.0, s.right() as f64)] {
            let gx = self.next_x(x0, side);
            let pts = [
                Pt::new(x0, y0),
                Pt::new(gx, y0),
                Pt::new(gx, above),
                Pt::new(b.x, above),
                b,
            ];
            if let Some(p) = self.take(elbow_h(0.0, &pts), s, t) {
                return p;
            }
        }
        s_curve()
    }

    /// The free column nearest `x` on the side `dir` points to.
    fn next_x(&self, x: f64, dir: f64) -> f64 {
        let mut best = x + dir * 10.0;
        let mut found = false;
        for &g in &self.xs {
            let side = if dir > 0.0 { g > x } else { g < x };
            if side && (!found || (g - x).abs() < (best - x).abs()) {
                (best, found) = (g, true);
            }
        }
        best
    }

    fn row_below(&self, y: f64) -> f64 {
        self.ys
            .iter()
            .copied()
            .filter(|&g| g > y)
            .min_by(f64::total_cmp)
            .unwrap_or(y + 14.0)
    }

    fn row_above(&self, y: f64) -> f64 {
        self.ys
            .iter()
            .copied()
            .filter(|&g| g < y)
            .max_by(f64::total_cmp)
            .unwrap_or(y - 14.0)
    }
}

/// Along its own row, then up or down into the called card.
fn along(s: &Bx, t: &Bx) -> Drawn {
    let (sx, sy, ex) = (s.right() as f64, s.cy() as f64, (t.cx() + 23) as f64);
    let ey = if s.cy() > t.bottom() { t.bottom() } else { t.y } as f64;
    elbow(&[Pt::new(sx, sy), Pt::new(ex, sy), Pt::new(ex, ey)])
}

/// The free lines between the cards: the middles of the runs no card covers, and one beyond
/// each side.
fn gaps(spans: impl Iterator<Item = (i64, i64)>) -> Vec<f64> {
    let mut spans: Vec<(i64, i64)> = spans.collect();
    spans.sort_by_key(|s| s.0);
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (a, b) in spans {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    let (Some(first), Some(last)) = (merged.first(), merged.last()) else {
        return Vec::new();
    };
    let mut out = vec![first.0 as f64 - 10.0];
    out.extend(merged.windows(2).filter_map(|w| match w {
        [p, q] => Some((p.1 + q.0) as f64 / 2.0),
        _ => None,
    }));
    out.push(last.1 as f64 + 12.0);
    out
}
