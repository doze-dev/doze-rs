//! The Go map's layout (flow.go), card for card, for small stacks: the apps left to right the way
//! a request travels, a column per step of calls; the apps nothing calls and that call nothing in
//! a lane of their own; what the stack shares in a band below, a card each, ordered under the
//! cards that use it. Its Go golden and three hundred generated stacks are its oracle.

mod order;
mod out;
mod route;

use crate::geom::{Pt, Rect};
use crate::input::{Flow, Instance};
use crate::output::{Caption, CaptionKind, Layout, Role};
use crate::text::{CARD_W, Lang, card_width};

const LEFT: i64 = 24; // the first column's left
const GAP: i64 = 72; // between columns: room for a line to bend and an arrowhead to land
const CARD_H: i64 = 74;
const STEP: i64 = 140; // card to card, down a column
const CAP_GAP: i64 = 36; // a caption sits this far above its cards
const SHARE_H: i64 = 88; // a band card
const SHARE_DY: i64 = 16; // between rows of the band
const MIN_W: i64 = 1222;
const PER_ROW: usize = 5; // band cards in a row, at least
const DETAIL_W: i64 = 448;
const DETAIL_H: i64 = 112;

/// A card's box in whole pixels, as the Go map kept them (its centres round down).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bx {
    pub(crate) x: i64,
    pub(crate) y: i64,
    pub(crate) w: i64,
    pub(crate) h: i64,
}

impl Bx {
    pub(crate) fn right(&self) -> i64 {
        self.x + self.w
    }
    pub(crate) fn bottom(&self) -> i64 {
        self.y + self.h
    }
    pub(crate) fn cx(&self) -> i64 {
        self.x + self.w / 2
    }
    pub(crate) fn cy(&self) -> i64 {
        self.y + self.h / 2
    }
    pub(crate) fn rect(&self) -> Rect {
        Rect::new(self.x as f64, self.y as f64, self.w as f64, self.h as f64)
    }
}

/// The grid of columns: each as wide as its widest card; a later part keeps the grid.
struct Grid {
    cols: Vec<(i64, i64)>,
    x: i64,
}

impl Grid {
    fn add(&mut self, w: i64) {
        self.cols.push((self.x, w));
        self.x += w + GAP;
    }
    fn at(&mut self, i: usize, w: i64) -> (i64, i64) {
        while self.cols.len() <= i {
            self.add(w);
        }
        self.cols.get(i).copied().unwrap_or((LEFT, w))
    }
}

/// The parts placed so far.
struct Board<'f, 'a> {
    f: &'f Flow<'a>,
    grid: Grid,
    /// Instance index to its box.
    at: Vec<Option<Bx>>,
    /// Placed cards, in the Go map's order: nodes, then the band.
    placed: Vec<(usize, Role)>,
    caps: Vec<Caption>,
    y: i64,
    band: Option<i64>,
}

/// Lays `ins` out as the Go map does.
pub(crate) fn layout(ins: &[Instance]) -> Layout {
    let f = Flow::read(ins);
    let (mut cols, lane) = order::order(&f);
    let (slot, rows) = order::arrange(&f, &mut cols);
    let mut b = Board {
        f: &f,
        grid: Grid {
            cols: Vec::new(),
            x: LEFT,
        },
        at: vec![None; ins.len()],
        placed: Vec::new(),
        caps: Vec::new(),
        y: 16,
        band: None,
    };
    for c in &cols {
        let w = c.iter().map(|&v| width(&f, v)).fold(CARD_W, i64::max);
        b.grid.add(w);
    }
    b.calls(&cols, &slot, rows);
    b.lane(&lane);
    b.shared();
    b.finish()
}

fn width(f: &Flow<'_>, v: usize) -> i64 {
    let run = f.inst(v).map_or("", |i| i.run.as_str());
    card_width(f.name(v), Lang::of(run))
}

fn caption(kind: CaptionKind, text: &str, sub: &str, x: i64, y: i64) -> Caption {
    Caption {
        kind,
        text: text.into(),
        sub: sub.into(),
        at: Pt::new(x as f64, y as f64),
    }
}

impl Board<'_, '_> {
    fn put(&mut self, v: usize, b: Bx, role: Role) {
        if let Some(a) = self.at.get_mut(v) {
            *a = Some(b);
        }
        self.placed.push((v, role));
    }

    fn calls(&mut self, cols: &[Vec<usize>], slot: &[Option<usize>], rows: usize) {
        if cols.is_empty() {
            return;
        }
        let sub = "left to right, the way a request travels";
        self.caps.push(caption(
            CaptionKind::Calls,
            "Who calls whom",
            sub,
            LEFT,
            self.y,
        ));
        let top = self.y + CAP_GAP;
        for (ci, c) in cols.iter().enumerate() {
            let (x, w) = self.grid.cols.get(ci).copied().unwrap_or((LEFT, CARD_W));
            for &v in c {
                let s = slot.get(v).copied().flatten().unwrap_or(0);
                let y = top + i64::try_from(s).unwrap_or(0) * STEP;
                self.put(v, Bx { x, y, w, h: CARD_H }, Role::Flow);
            }
        }
        self.y = top + i64::try_from(rows).unwrap_or(0) * STEP - (STEP - CARD_H) + 14;
    }

    fn lane(&mut self, lane: &[usize]) {
        if lane.is_empty() {
            return;
        }
        let events = lane.iter().all(|&a| self.f.on_events(a));
        let (text, sub) = if events {
            ("Nobody calls these", "they work from events")
        } else {
            ("No callers", "nothing in doze.hcl refers to these")
        };
        self.caps
            .push(caption(CaptionKind::Lane, text, sub, LEFT, self.y));
        let flow = self.placed.iter().any(|(_, r)| *r == Role::Flow);
        let top = self.y + CAP_GAP - if flow { 12 } else { 0 };
        for (i, &a) in lane.iter().enumerate() {
            let (x, w) = self.grid.at(i, CARD_W);
            self.put(
                a,
                Bx {
                    x,
                    y: top,
                    w,
                    h: CARD_H,
                },
                Role::Lane,
            );
        }
        self.y = top + CARD_H;
    }

    /// The band: what the stack shares, ordered under the cards that use it.
    fn shared(&mut self) {
        let f = self.f;
        if f.shared.is_empty() {
            return;
        }
        let pull = |s: usize| {
            let users = f.users(s);
            if users.is_empty() {
                return 1e9;
            }
            let at = |u: usize| self.at.get(u).copied().flatten().map_or(0, |b| b.cx()) as f64;
            users.iter().map(|&u| at(u)).sum::<f64>() / users.len() as f64
        };
        let mut shared: Vec<(f64, usize)> = f.shared.iter().map(|&s| (pull(s), s)).collect();
        shared
            .sort_by(|(pa, a), (pb, b)| pa.total_cmp(pb).then_with(|| f.name(*a).cmp(f.name(*b))));
        let band = self.y + 16;
        self.band = Some(band);
        let mut sub = String::from("the chips on each card say who uses what");
        let n = f.uses_all();
        if n > 1 {
            sub = format!("one line each would be {n} lines; {sub}");
        }
        self.caps.push(caption(
            CaptionKind::Band,
            "Shared by the stack",
            &sub,
            LEFT,
            band + 10,
        ));
        let per_row = self.grid.cols.len().max(PER_ROW);
        let top = band + 36;
        for (i, &(_, s)) in shared.iter().enumerate() {
            let (r, c) = (i64::try_from(i / per_row).unwrap_or(0), i % per_row);
            let (x, w) = self.grid.at(c, CARD_W);
            self.put(
                s,
                Bx {
                    x,
                    y: top + r * (SHARE_H + SHARE_DY),
                    w,
                    h: SHARE_H,
                },
                Role::Shared,
            );
        }
        let rows = i64::try_from((shared.len() - 1) / per_row + 1).unwrap_or(1);
        self.y = top + rows * (SHARE_H + SHARE_DY) - SHARE_DY;
    }

    fn finish(self) -> Layout {
        let boxes: Vec<Bx> = self
            .placed
            .iter()
            .filter_map(|(v, _)| self.at.get(*v).copied().flatten())
            .collect();
        let h = self.y + 16;
        let w = boxes.iter().map(|b| b.right() + 12).fold(MIN_W, i64::max);
        let detail = detail_at(&boxes, w, h);
        let parts = out::Parts {
            f: self.f,
            placed: self.placed,
            at: self.at,
            caps: self.caps,
            band: self.band,
            size: (w, h),
            detail,
        };
        parts.build()
    }
}

/// The detail card: top right where that is clear of the cards, else the first clear place down
/// the right side, else top right over them.
fn detail_at(cards: &[Bx], w: i64, h: i64) -> Pt {
    let x = w - 12 - DETAIL_W;
    let clear = |y: i64| {
        let d = Bx {
            x,
            y,
            w: DETAIL_W,
            h: DETAIL_H,
        };
        !cards.iter().any(|c| {
            d.x < c.right() + 8
                && c.x < d.right() + 8
                && d.y < c.bottom() + 8
                && c.y < d.bottom() + 8
        })
    };
    let y = (0..)
        .map(|k| 14 + 8 * k)
        .take_while(|y| y + DETAIL_H < h)
        .find(|&y| clear(y))
        .unwrap_or(14);
    Pt::new(x as f64, y as f64)
}

pub(crate) use order::{back_edges, depths, split};
pub(crate) use route::Router;
