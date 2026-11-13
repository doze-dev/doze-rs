//! The Map's scene, painted directly on one canvas: the band, the regions and bundles far out, the calls in view, the
//! cards as blocks when zoomed out, the focus card's own lines and the selection's dashed use lines.
//!
//! What a call looks like by level (the fix for the whole-map hairball: thousands of calls drawn at full strength):
//!
//! | zoom | small map (Go's layout) | large map |
//! |---|---|---|
//! | under [`LABEL_ZOOM`] | — (a small map is never this far out) | regions tinted, calls between them bundled and weighted; single calls not drawn |
//! | to [`DETAIL_ZOOM`] | polylines | polylines, faint |
//! | above | every call at full strength (Go's look) | every call, half strength |
//!
//! At every level the focus card (selected, else hovered) has its own calls drawn on top in the accent.

use std::rc::Rc;
use std::sync::Arc;

use doze_map::{CardId, Layout, Mode, Overview, Rect, Role, Seg, Visible};
use gpui::{
    AnyElement, BorderStyle, Bounds, Hsla, PathBuilder, Pixels, Point, Window, canvas, point,
    prelude::*, px, quad, size,
};

use super::cache::{Cache, build, level, place};
use super::{DETAIL_ZOOM, LABEL_ZOOM, MapView};
use crate::theme::{Tokens, alpha, mix};

/// What one frame of the scene paints with.
#[derive(Clone)]
struct Scene {
    t: Tokens,
    l: Arc<Layout>,
    o: Rc<Overview>,
    cache: Rc<Cache>,
    states: Rc<Vec<u8>>,
    vis: Visible,
    view: Rect,
    z: f32,
    pan: (f32, f32),
    lit: Option<CardId>,
    focus: Option<CardId>,
    on: Option<Rc<Vec<bool>>>,
}

/// Which cards a selection keeps lit: the card, its calls, callers and uses.
pub(crate) fn lit_set(l: &Layout, lit: Option<CardId>) -> Option<Vec<bool>> {
    let id = lit?;
    let c = l.card(id)?;
    let mut on = vec![false; l.cards.len()];
    for x in std::iter::once(&id)
        .chain(&c.calls)
        .chain(&c.callers)
        .chain(&c.uses)
    {
        if let Some(b) = on.get_mut(x.ix()) {
            *b = true;
        }
    }
    Some(on)
}

/// Whether the map is big enough that far out it is drawn as regions and bundles.
pub(crate) fn bundled(m: &MapView) -> bool {
    m.layout.mode == Mode::Layered && m.cam.zoom < LABEL_ZOOM
}

/// The scene element: fills its parent, and records the viewport (window position and size) in `viewport`.
pub(crate) fn scene(
    t: &Tokens,
    m: &MapView,
    vis: &Visible,
    viewport: Rc<std::cell::Cell<(f32, f32, f32, f32)>>,
) -> AnyElement {
    let s = Scene {
        t: *t,
        l: m.layout.clone(),
        o: m.overview.clone(),
        cache: m.cache.clone(),
        states: m.states.clone(),
        vis: vis.clone(),
        view: m.cam.view(),
        z: m.cam.zoom,
        pan: m.cam.pan,
        lit: m.lit,
        focus: m.focus_card(),
        on: lit_set(&m.layout, m.lit).map(Rc::new),
    };
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| s.paint(bounds, window, &viewport),
    )
    .absolute()
    .top(px(0.))
    .left(px(0.))
    .size_full()
    .into_any_element()
}

impl Scene {
    fn paint(
        &self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        viewport: &std::cell::Cell<(f32, f32, f32, f32)>,
    ) {
        let vp = (
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        );
        if viewport.get() != vp {
            // The first frame, or a resize: the camera learns its size and the next frame fits or keeps up.
            viewport.set(vp);
            window.refresh();
        }
        self.cache.begin();
        let o = point(
            bounds.origin.x - px(self.pan.0 * self.z),
            bounds.origin.y - px(self.pan.1 * self.z),
        );
        self.band(bounds, o, window);
        let far = self.l.mode == Mode::Layered && self.z < LABEL_ZOOM;
        if far {
            self.regions(o, window);
            self.bundles(o, window);
        } else {
            self.lines(o, window);
        }
        if self.z < LABEL_ZOOM {
            self.far_blocks(o, window);
        } else if self.z < DETAIL_ZOOM {
            self.blocks(o, window);
        }
        self.focus_lines(o, window);
        if let Some(c) = self.lit {
            self.uses(c, o, window);
        }
        if self.cache.stale.get() {
            window.refresh();
        }
    }

    fn at(&self, o: Point<Pixels>, x: f64, y: f64) -> Point<Pixels> {
        point(o.x + px(x as f32 * self.z), o.y + px(y as f32 * self.z))
    }

    fn rect(&self, o: Point<Pixels>, r: Rect) -> Bounds<Pixels> {
        Bounds {
            origin: self.at(o, r.x, r.y),
            size: size(px(r.w as f32 * self.z), px(r.h as f32 * self.z)),
        }
    }

    /// The shared band: a quieter floor under what the stack shares.
    fn band(&self, bounds: Bounds<Pixels>, o: Point<Pixels>, window: &mut Window) {
        let Some(band) = self.l.band else { return };
        let top = self.at(o, 0.0, band).y.max(bounds.origin.y);
        let h = (bounds.origin.y + bounds.size.height - top).max(px(0.));
        let b = Bounds {
            origin: point(bounds.origin.x, top),
            size: size(bounds.size.width, h),
        };
        window.paint_quad(gpui::fill(b, alpha(self.t.panel, 0.45)));
        window.paint_quad(gpui::fill(
            Bounds {
                origin: b.origin,
                size: size(b.size.width, px(1.)),
            },
            self.t.line2,
        ));
    }

    /// Far out in a large map: each region a soft tint, so the bundles have somewhere to land (no outline: regions
    /// may touch, and outlines there read as clutter).
    fn regions(&self, o: Point<Pixels>, window: &mut Window) {
        let fill = alpha(self.t.accent, if self.t.dark { 0.045 } else { 0.06 });
        for r in &self.o.regions {
            let pad = 8.0 / f64::from(self.z);
            let b = self.rect(
                o,
                Rect::new(
                    r.rect.x - pad,
                    r.rect.y - pad,
                    r.rect.w + 2.0 * pad,
                    r.rect.h + 2.0 * pad,
                ),
            );
            window.paint_quad(gpui::fill(b, fill).corner_radii(px(6.)));
        }
    }

    /// Far out in a large map: the bundles, as wide as their weight; faded under a selection, except its region's.
    fn bundles(&self, o: Point<Pixels>, window: &mut Window) {
        let lv = level(self.z);
        let home = self
            .focus
            .and_then(|c| self.o.region_of.get(c.ix()).copied().flatten());
        let heaviest = self.o.heaviest() as f32;
        let quiet = alpha(self.t.dim, 0.14);
        self.cache.bundles(&self.o, lv, &mut |p, i, at| {
            let Some(b) = self.o.bundles.get(i) else {
                return;
            };
            // Heavier bundles stronger: a single call between regions is a whisper, the main flows carry the picture.
            let weight = (b.count as f32 / heaviest).sqrt();
            let strength = if b.count <= 1 {
                0.12
            } else {
                0.22 + 0.58 * weight
            };
            let mine = home.is_some_and(|h| b.from == h || b.to == h);
            let color = match home {
                Some(_) if !mine => quiet,
                _ => alpha(self.t.accent, strength),
            };
            window.paint_path(place(p, self.z / at, o), color);
        });
    }

    /// Single calls: batched polylines up to [`DETAIL_ZOOM`], curves with heads above it.
    fn lines(&self, o: Point<Pixels>, window: &mut Window) {
        let lv = level(self.z);
        let large = self.l.mode == Mode::Layered;
        let strength = if large {
            if self.z < DETAIL_ZOOM { 0.2 } else { 0.55 }
        } else {
            1.0
        };
        let faded = alpha(self.t.line2, 0.6);
        if lv < DETAIL_ZOOM {
            let c = if self.lit.is_some() {
                faded
            } else {
                alpha(self.t.accent, strength * 0.8)
            };
            self.cache.chunks(&self.l, self.view, lv, &mut |p, at| {
                window.paint_path(place(p, self.z / at, o), c)
            });
            return;
        }
        for &e in &self.vis.edges {
            let Some(edge) = self.l.edges.get(e) else {
                continue;
            };
            let theirs = self.lit.is_some_and(|c| edge.from == c || edge.to == c);
            let color = match self.lit {
                None => alpha(self.t.accent, strength),
                Some(_) if theirs => self.t.accent,
                Some(_) => faded,
            };
            self.cache.line(&self.l, e, lv, &mut |p, at| {
                window.paint_path(place(p, self.z / at, o), color)
            });
        }
    }

    /// The focus card's own calls, on top of everything, at full strength: curves with heads at any zoom, built
    /// fresh at the exact zoom (a card has a handful), at least 2 px wide and with a head that still shows far out.
    fn focus_lines(&self, o: Point<Pixels>, window: &mut Window) {
        let Some(c) = self.focus else { return };
        let z = self.z;
        let width = 1.5_f32.max(2.0 / z);
        let head = 1.0_f64.max(0.7 / f64::from(z));
        for edge in self.l.edges.iter().filter(|e| e.from == c || e.to == c) {
            for p in build(&edge.path, z, width, head) {
                window.paint_path(place(&p, 1.0, o), self.t.accent);
            }
        }
    }

    fn state_color(&self, i: usize) -> Hsla {
        let pal = [
            self.t.good,
            self.t.warn,
            self.t.dim,
            self.t.cool,
            self.t.bad,
        ];
        let s = usize::from(self.states.get(i).copied().unwrap_or(2));
        pal.get(s).copied().unwrap_or(self.t.dim)
    }

    /// Between far and close: each card in view a block with a state strip.
    fn blocks(&self, o: Point<Pixels>, window: &mut Window) {
        for &c in &self.vis.cards {
            let Some(card) = self.l.card(c) else { continue };
            let b = self.rect(o, card.rect);
            let faded = self
                .on
                .as_ref()
                .is_some_and(|on| !on.get(c.ix()).copied().unwrap_or(false));
            let bg = if card.role == Role::Shared {
                mix(self.t.accent, self.t.panel, 8.)
            } else {
                self.t.panel
            };
            let border = if self.focus == Some(c) {
                self.t.accent
            } else {
                self.t.line2
            };
            let a = if faded { 0.3 } else { 1.0 };
            let radius = px((11.0 * self.z).max(1.0));
            let bw = if b.size.height > px(6.) {
                px(1.)
            } else {
                px(0.)
            };
            window.paint_quad(quad(
                b,
                radius,
                alpha(bg, a),
                bw,
                alpha(border, a),
                BorderStyle::Solid,
            ));
            let strip = Bounds {
                origin: b.origin,
                size: size((b.size.width * 0.05).max(px(2.)), b.size.height),
            };
            window.paint_quad(quad(
                strip,
                radius,
                alpha(self.state_color(c.ix()), a),
                px(0.),
                self.t.dim,
                BorderStyle::Solid,
            ));
            if card.members > 1 && b.size.height > px(8.) {
                // A collapsed group: a second edge behind, like a stack of cards.
                let back = Bounds {
                    origin: point(b.origin.x + px(3.), b.origin.y + b.size.height),
                    size: size(b.size.width - px(6.), px(2.)),
                };
                window.paint_quad(gpui::fill(back, alpha(self.t.line2, a)));
            }
        }
    }

    /// Far out: every block from cached batches (a few dozen paths for thousands of cards); the focus card and a
    /// selection's cards again on top.
    fn far_blocks(&self, o: Point<Pixels>, window: &mut Window) {
        let lv = level(self.z);
        let a = if self.on.is_some() { 0.35 } else { 1.0 };
        let colors = [
            mix(self.t.dim, self.t.panel, 35.),
            mix(self.t.accent, self.t.panel, 30.),
        ];
        self.cache
            .blocks(&self.l, &self.states, self.view, lv, &mut |p, color, at| {
                let c = if color < 2 {
                    colors.get(color).copied().unwrap_or(self.t.dim)
                } else {
                    self.state_color_of(color - 2)
                };
                window.paint_path(place(p, self.z / at, o), alpha(c, a));
            });
        let mut mark = |i: usize, color: Hsla| {
            if let Some(c) = self.l.cards.get(i) {
                let b = self.rect(o, c.rect);
                let b = Bounds {
                    origin: b.origin,
                    size: size(b.size.width.max(px(3.)), b.size.height.max(px(3.))),
                };
                window.paint_quad(gpui::fill(b, color));
            }
        };
        if let Some(on) = &self.on {
            on.iter()
                .enumerate()
                .filter(|(_, b)| **b)
                .for_each(|(i, _)| mark(i, self.t.accent));
        }
        if let Some(f) = self.focus {
            mark(f.ix(), self.t.bright);
        }
    }

    fn state_color_of(&self, state: usize) -> Hsla {
        [
            self.t.good,
            self.t.warn,
            self.t.dim,
            self.t.cool,
            self.t.bad,
        ]
        .get(state)
        .copied()
        .unwrap_or(self.t.dim)
    }

    /// The selection's dashed lines to what it uses, each ending in a dot.
    fn uses(&self, card: CardId, o: Point<Pixels>, window: &mut Window) {
        let z = self.z;
        let w = (2.0 * z).max(1.0);
        for (_, _, p) in self.l.use_lines(card) {
            let mut b = PathBuilder::stroke(px(w))
                .dash_array(&[px((5.0 * z).max(2.)), px((4.0 * z).max(2.))]);
            for s in &p.segs {
                match *s {
                    Seg::Move(q) => b.move_to(self.at(o, q.x, q.y)),
                    Seg::Line(q) => b.line_to(self.at(o, q.x, q.y)),
                    Seg::Cubic(a, c, q) => b.cubic_bezier_to(
                        self.at(o, q.x, q.y),
                        self.at(o, a.x, a.y),
                        self.at(o, c.x, c.y),
                    ),
                    Seg::Quad(c, q) => b.curve_to(self.at(o, q.x, q.y), self.at(o, c.x, c.y)),
                    _ => {}
                }
            }
            if let Ok(path) = b.build() {
                window.paint_path(path, self.t.accent);
            }
            if let Some((end, _)) = p.tip() {
                dot(
                    window,
                    self.at(o, end.x, end.y),
                    (3.5 * z).max(2.0),
                    self.t.accent,
                );
            }
        }
    }
}

/// A filled disc.
fn dot(window: &mut Window, c: Point<Pixels>, r: f32, color: Hsla) {
    let pts: Vec<Point<Pixels>> = (0..16)
        .map(|i| {
            let a = i as f32 / 16.0 * std::f32::consts::TAU;
            point(c.x + px(r * a.cos()), c.y + px(r * a.sin()))
        })
        .collect();
    let mut f = PathBuilder::fill();
    f.add_polygon(&pts, true);
    if let Ok(path) = f.build() {
        window.paint_path(path, color);
    }
}
