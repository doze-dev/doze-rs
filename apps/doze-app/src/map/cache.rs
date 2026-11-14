//! Tessellated geometry, kept across frames. Tessellating a curve is the expensive part of a frame, so each line is
//! built once per zoom level (powers of two) and only moved and scaled after: panning costs a copy of each visible
//! line's vertices, and a pinch re-tessellates only when it crosses a level. A level change does not stall a frame:
//! what is not yet built at the new level is drawn from the level it has, scaled, and rebuilt within a per-frame
//! budget, so a pinch refines as it goes. (The spike's measured design: frame build p95 ≤ 4.7 ms at 2,000 cards.)

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::time::{Duration, Instant};

use doze_map::{Layout, Overview, Path as MapPath, Rect, Role, Seg};
use gpui::{Bounds, PathBuilder, Pixels, Point, point, px, size};

use super::DETAIL_ZOOM;

pub(crate) type GPath = gpui::Path<Pixels>;

/// Tessellation a frame may spend on pieces it could draw scaled from another level.
const BUDGET: Duration = Duration::from_millis(3);
/// Lines per piece far out, cards per piece of blocks.
const LINES: usize = 256;
const CARDS: usize = 512;

/// What is kept.
#[derive(Default)]
pub(crate) struct Cache {
    inner: RefCell<Inner>,
    deadline: Cell<Option<Instant>>,
    /// A frame drew geometry from another level: it asks for another frame to refine.
    pub(crate) stale: Cell<bool>,
}

#[derive(Default)]
struct Inner {
    lines: HashMap<usize, (f32, Vec<GPath>)>,
    chunks: Vec<Piece>,
    blocks: Vec<Piece>,
    bundles: Option<(f32, Vec<(usize, GPath)>)>,
    mini: Option<(f32, Vec<(usize, GPath)>)>,
}

/// A run of lines or cards (by index) drawn as a few paths, one per colour.
struct Piece {
    bbox: Rect,
    items: Range<usize>,
    built: Option<(f32, Vec<(usize, GPath)>)>,
}

fn pieces(boxes: impl Iterator<Item = Rect>, per: usize) -> Vec<Piece> {
    let boxes: Vec<Rect> = boxes.collect();
    boxes
        .chunks(per)
        .enumerate()
        .filter_map(|(i, c)| {
            let bbox = c.iter().skip(1).fold(*c.first()?, |a, r| a.union(r));
            Some(Piece {
                bbox,
                items: i * per..i * per + c.len(),
                built: None,
            })
        })
        .collect()
}

/// Geometry built at `built` may stand in, scaled, at `level` only this close; further, widths would be wrong.
fn near(built: f32, level: f32) -> bool {
    (0.5..=2.0).contains(&(level / built))
}

/// The zoom level geometry is built at: the zoom's nearest power of two.
pub(crate) fn level(z: f32) -> f32 {
    2f32.powf(z.log2().round())
}

/// What a draw of pieces needs.
struct Draw<'a> {
    view: Rect,
    level: f32,
    cache: &'a Cache,
}

impl Draw<'_> {
    fn run(
        &self,
        ps: &mut [Piece],
        build: &dyn Fn(Range<usize>) -> Vec<(usize, GPath)>,
        f: &mut dyn FnMut(&GPath, usize, f32),
    ) {
        for p in ps.iter_mut().filter(|p| p.bbox.intersects(&self.view)) {
            if p.built
                .as_ref()
                .is_none_or(|b| b.0 != self.level && (self.cache.fresh() || !near(b.0, self.level)))
            {
                p.built = Some((self.level, build(p.items.clone())));
            }
            if let Some((lv, paths)) = &p.built {
                if *lv != self.level {
                    self.cache.stale.set(true);
                }
                for (c, g) in paths {
                    f(g, *c, *lv);
                }
            }
        }
    }
}

/// Rectangles as filled paths, one per colour.
fn rects(rs: &[(Rect, usize)], scale: f32) -> Vec<(usize, GPath)> {
    let colors = rs.iter().map(|r| r.1).max().map_or(0, |m| m + 1);
    (0..colors)
        .filter_map(|color| {
            let mut b = PathBuilder::fill();
            let mut any = false;
            for (r, _) in rs.iter().filter(|r| r.1 == color) {
                let (x0, y0) = (r.x as f32 * scale, r.y as f32 * scale);
                let (x1, y1) = (r.right() as f32 * scale, r.bottom() as f32 * scale);
                b.add_polygon(
                    &[
                        point(px(x0), px(y0)),
                        point(px(x1), px(y0)),
                        point(px(x1), px(y1)),
                        point(px(x0), px(y1)),
                    ],
                    true,
                );
                any = true;
            }
            any.then(|| b.build().ok().map(|p| (color, p))).flatten()
        })
        .collect()
}

impl Cache {
    /// Forgets everything (a new layout, or new states far out).
    pub(crate) fn clear(&self) {
        *self.inner.borrow_mut() = Inner::default();
    }

    /// Starts a frame's budget.
    pub(crate) fn begin(&self) {
        self.deadline.set(Some(Instant::now() + BUDGET));
        self.stale.set(false);
    }

    fn fresh(&self) -> bool {
        self.deadline.get().is_none_or(|d| Instant::now() < d)
    }

    /// Far out: card bodies (0 app, 1 shared) and state strips (2 + state).
    pub(crate) fn blocks(
        &self,
        l: &Layout,
        states: &[u8],
        view: Rect,
        lv: f32,
        f: &mut dyn FnMut(&GPath, usize, f32),
    ) {
        let mut inner = self.inner.borrow_mut();
        if inner.blocks.is_empty() {
            inner.blocks = pieces(l.cards.iter().map(|c| c.rect), CARDS);
        }
        let build = |range: Range<usize>| {
            let mut rs = Vec::with_capacity(range.len() * 2);
            for i in range {
                let Some(c) = l.cards.get(i) else { continue };
                if l.folded(doze_map::CardId(u32::try_from(i).unwrap_or(u32::MAX))) {
                    continue;
                }
                let r = c.rect;
                rs.push((r, usize::from(c.role == Role::Shared)));
                let strip = Rect::new(r.x, r.y, (r.w * 0.05).max(2.0 / f64::from(lv)), r.h);
                rs.push((strip, 2 + usize::from(states.get(i).copied().unwrap_or(2))));
            }
            rects(&rs, lv)
        };
        Draw {
            view,
            level: lv,
            cache: self,
        }
        .run(&mut inner.blocks, &build, f);
    }

    /// Far out: every line, in pieces, as polylines through its turns.
    pub(crate) fn chunks(&self, l: &Layout, view: Rect, lv: f32, f: &mut dyn FnMut(&GPath, f32)) {
        let mut inner = self.inner.borrow_mut();
        if inner.chunks.is_empty() {
            inner.chunks = pieces(l.edges.iter().map(|e| e.path.bbox()), LINES);
        }
        let build = |range: Range<usize>| {
            let pt = |p: doze_map::Pt| point(px(p.x as f32 * lv), px(p.y as f32 * lv));
            let mut b = PathBuilder::stroke(px(1.0));
            // Calls between regions are drawn on their own ([`Cache::line`]), and only when asked.
            for e in range.filter_map(|i| l.edges.get(i)).filter(|e| !e.across) {
                for (i, q) in e.path.waypoints().into_iter().enumerate() {
                    if i == 0 {
                        b.move_to(pt(q))
                    } else {
                        b.line_to(pt(q))
                    }
                }
            }
            b.build().ok().map(|p| vec![(0, p)]).unwrap_or_default()
        };
        Draw {
            view,
            level: lv,
            cache: self,
        }
        .run(&mut inner.chunks, &build, &mut |p, _, at| f(p, at));
    }

    /// Up close: one line, built at `lv` (or drawn from the level it has, while the frame's budget is spent).
    pub(crate) fn line(&self, l: &Layout, e: usize, lv: f32, f: &mut dyn FnMut(&GPath, f32)) {
        let mut inner = self.inner.borrow_mut();
        let stale = inner.lines.get(&e).is_none_or(|b| b.0 != lv);
        let far = inner.lines.get(&e).is_none_or(|b| !near(b.0, lv));
        if stale && (far || self.fresh()) {
            let built = l
                .edges
                .get(e)
                .map(|x| build(&x.path, lv, 1.5, 1.0))
                .unwrap_or_default();
            inner.lines.insert(e, (lv, built));
        }
        if let Some((at, paths)) = inner.lines.get(&e) {
            if *at != lv {
                self.stale.set(true);
            }
            for p in paths {
                f(p, *at);
            }
        }
    }

    /// Far out: the overview's bundles, each as wide as its weight up to `widest` screen pixels more than a single
    /// call's (index into `o.bundles`), built once per level.
    pub(crate) fn bundles(
        &self,
        o: &Overview,
        lv: f32,
        widest: f32,
        f: &mut dyn FnMut(&GPath, usize, f32),
    ) {
        let mut inner = self.inner.borrow_mut();
        if inner.bundles.as_ref().is_none_or(|b| !near(b.0, lv)) {
            let heaviest = o.heaviest() as f32;
            let built = o
                .bundles
                .iter()
                .enumerate()
                .flat_map(|(i, b)| {
                    // Screen pixels: 1.25 for one call up to 9.25 for the heaviest; the head grows with the line.
                    let w = 1.25 + widest * (b.count as f32 / heaviest).sqrt();
                    build(&b.path, lv, w / lv, f64::from((0.45 + w / 10.0) / lv))
                        .into_iter()
                        .map(move |p| (i, p))
                })
                .collect();
            inner.bundles = Some((lv, built));
        }
        if let Some((at, paths)) = &inner.bundles {
            for (i, p) in paths {
                f(p, *i, *at);
            }
        }
    }

    /// The minimap's cards (0 app, 1 shared) at its scale, built once.
    pub(crate) fn mini(&self, l: &Layout, scale: f32, f: &mut dyn FnMut(&GPath, usize)) {
        let mut inner = self.inner.borrow_mut();
        if inner.mini.as_ref().is_none_or(|b| b.0 != scale) {
            let rs: Vec<(Rect, usize)> = l
                .cards
                .iter()
                .enumerate()
                .filter(|(i, _)| !l.folded(doze_map::CardId(u32::try_from(*i).unwrap_or(u32::MAX))))
                .map(|(_, c)| {
                    let r = c.rect;
                    // At least a pixel each way, so a card never vanishes from the overview.
                    let (w, h) = (
                        r.w.max(1.0 / f64::from(scale)),
                        r.h.max(1.0 / f64::from(scale)),
                    );
                    (
                        Rect::new(r.x, r.y, w, h),
                        usize::from(c.role == Role::Shared),
                    )
                })
                .collect();
            inner.mini = Some((scale, rects(&rs, scale)));
        }
        if let Some((_, bs)) = &inner.mini {
            for (c, p) in bs {
                f(p, *c);
            }
        }
    }
}

/// A line at a level: the curve and its head up close (the stroke `width` map pixels wide, the head `head` times a
/// call's), a polyline far out.
pub(crate) fn build(path: &MapPath, lv: f32, width: f32, head: f64) -> Vec<GPath> {
    let pt = |p: doze_map::Pt| point(px(p.x as f32 * lv), px(p.y as f32 * lv));
    let mut out = Vec::new();
    if lv < DETAIL_ZOOM && width <= 1.5 {
        let mut b = PathBuilder::stroke(px(1.0));
        for (i, p) in path.waypoints().into_iter().enumerate() {
            if i == 0 {
                b.move_to(pt(p))
            } else {
                b.line_to(pt(p))
            }
        }
        out.extend(b.build().ok());
        return out;
    }
    let w = width * lv;
    let opts = gpui::StrokeOptions::default()
        .with_line_width(w)
        .with_line_cap(lyon::tessellation::LineCap::Round)
        .with_line_join(lyon::tessellation::LineJoin::Round);
    let mut b = PathBuilder::stroke(px(w)).with_style(gpui::PathStyle::Stroke(opts));
    for s in &path.segs {
        match *s {
            Seg::Move(p) => b.move_to(pt(p)),
            Seg::Line(p) => b.line_to(pt(p)),
            Seg::Cubic(a, c, p) => b.cubic_bezier_to(pt(p), pt(a), pt(c)),
            Seg::Quad(c, p) => b.curve_to(pt(p), pt(c)),
            _ => {}
        }
    }
    out.extend(b.build().ok());
    if let Some((end, dir)) = path.tip() {
        let k = head;
        let local = [(0.5 * k, -4.2 * k), (9.5 * k, 0.), (0.5 * k, 4.2 * k)];
        let pts: Vec<Point<Pixels>> = local
            .iter()
            .map(|(lx, ly)| {
                pt(doze_map::Pt::new(
                    end.x + dir.x * lx - dir.y * ly,
                    end.y + dir.y * lx + dir.x * ly,
                ))
            })
            .collect();
        let mut fill = PathBuilder::fill();
        fill.add_polygon(&pts, true);
        out.extend(fill.build().ok());
    }
    out
}

/// A cached path moved to the screen: scaled from its level to the zoom, then offset.
pub(crate) fn place(p: &GPath, s: f32, o: Point<Pixels>) -> GPath {
    let mut q = p.clone();
    let tr = |v: Point<Pixels>| point(px(f32::from(v.x) * s) + o.x, px(f32::from(v.y) * s) + o.y);
    for v in &mut q.vertices {
        v.xy_position = tr(v.xy_position);
    }
    q.bounds = Bounds {
        origin: tr(q.bounds.origin),
        size: size(q.bounds.size.width * s, q.bounds.size.height * s),
    };
    q
}
