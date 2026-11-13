//! The scene's far and framed parts: the regions' tints and frames, the bundles between them, and every card as a
//! block from cached batches when zoomed far out.

use doze_map::{CardId, Rect};
use gpui::{BorderStyle, Bounds, Hsla, Pixels, Point, Window, px, quad, size};

use super::Scene;
use crate::map::cache::{level, place};
use crate::theme::{alpha, mix};

impl Scene {
    /// Far out in a large map: each region a soft tint, so the bundles have somewhere to land (no outline: regions
    /// may touch, and outlines there read as clutter).
    pub(super) fn regions(&self, o: Point<Pixels>, window: &mut Window) {
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
    pub(super) fn bundles(&self, o: Point<Pixels>, window: &mut Window) {
        let lv = level(self.z);
        let home = self
            .focus
            .and_then(|c| self.o.region_of.get(c.ix()).copied().flatten());
        let heaviest = self.o.heaviest() as f32;
        let quiet = alpha(self.t.dim, 0.14);
        // Between named regions the bundles stand in for the calls at every zoom: quieter and thinner than the
        // far-out overview's, so the frames and cards stay what the eye reads first.
        let named = !self.l.regions.is_empty();
        let widest = if named { 2.5 } else { 8.0 };
        self.cache.bundles(&self.o, lv, widest, &mut |p, i, at| {
            let Some(b) = self.o.bundles.get(i) else {
                return;
            };
            // Heavier bundles stronger: a single call between regions is a whisper, the main flows carry the picture.
            let weight = (b.count as f32 / heaviest).sqrt();
            let strength = match (named, b.count) {
                (true, 0..=1) => 0.16,
                (true, _) => 0.18 + 0.24 * weight,
                (false, 0..=1) => 0.12,
                (false, _) => 0.22 + 0.58 * weight,
            };
            let mine = home.is_some_and(|h| b.from == h || b.to == h);
            let color = match home {
                Some(_) if !mine => quiet,
                _ => alpha(self.t.accent, strength),
            };
            window.paint_path(place(p, self.z / at, o), color);
        });
    }

    /// A layered map's named regions: each a quiet frame, the focus card's outlined in the accent.
    pub(super) fn frames(&self, o: Point<Pixels>, window: &mut Window) {
        let fill = alpha(self.t.accent, if self.t.dark { 0.035 } else { 0.045 });
        let radius = px((14.0 * self.z).max(3.0));
        for (k, r) in self.l.regions.iter().enumerate() {
            if !r.rect.intersects(&self.view) {
                continue;
            }
            let border = if self.home == Some(k) {
                alpha(self.t.accent, 0.6)
            } else {
                self.t.line2
            };
            let b = self.rect(o, r.rect);
            window.paint_quad(quad(b, radius, fill, px(1.), border, BorderStyle::Solid));
        }
    }

    /// Far out: every block from cached batches (a few dozen paths for thousands of cards); the focus card and a
    /// selection's cards again on top.
    pub(super) fn far_blocks(&self, o: Point<Pixels>, window: &mut Window) {
        let lv = level(self.z);
        let dim = |on: bool| if on { 0.35 } else { 1.0 };
        let from = dim(self.fades.on_from.is_some());
        let a = from + (dim(self.on.is_some()) - from) * self.pl;
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
            if let Some(c) = self
                .l
                .cards
                .get(i)
                .filter(|_| !self.l.folded(CardId(i as u32)))
            {
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

    pub(super) fn state_color_of(&self, state: usize) -> Hsla {
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
}
