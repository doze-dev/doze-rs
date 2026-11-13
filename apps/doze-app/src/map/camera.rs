//! The Map's camera: screen pixels per map pixel and the map point at the viewport's top left, fitted, zoomed about a
//! point, panned, kept over the drawing. Plain arithmetic, tested without a window.

use doze_map::{CardId, Layout, Mode, Rect};

/// The furthest out and closest in.
pub(crate) const MIN_ZOOM: f32 = 0.02;
pub(crate) const MAX_ZOOM: f32 = 2.5;

/// Where the camera is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Camera {
    pub(crate) zoom: f32,
    pub(crate) pan: (f32, f32),
    /// The viewport's size in window pixels, as last painted.
    pub(crate) size: (f32, f32),
    /// The first fit has happened.
    pub(crate) fitted: bool,
    /// A move in progress ([`super::flight`]).
    pub(crate) flight: Option<super::flight::Flight>,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            zoom: 1.0,
            pan: (0.0, 0.0),
            size: (0.0, 0.0),
            fitted: false,
            flight: None,
        }
    }
}

impl Camera {
    fn wh(&self) -> (f32, f32) {
        (self.size.0.max(1.0), self.size.1.max(1.0))
    }

    /// The map rectangle in view.
    pub(crate) fn view(&self) -> Rect {
        let (w, h) = self.wh();
        Rect::new(
            f64::from(self.pan.0),
            f64::from(self.pan.1),
            f64::from(w / self.zoom),
            f64::from(h / self.zoom),
        )
    }

    /// A map point in viewport pixels.
    pub(crate) fn to_screen(self, x: f64, y: f64) -> (f32, f32) {
        (
            (x as f32 - self.pan.0) * self.zoom,
            (y as f32 - self.pan.1) * self.zoom,
        )
    }

    /// A viewport point on the map.
    pub(crate) fn to_map(self, sx: f32, sy: f32) -> doze_map::Pt {
        doze_map::Pt::new(
            f64::from(self.pan.0 + sx / self.zoom),
            f64::from(self.pan.1 + sy / self.zoom),
        )
    }

    /// The whole drawing in view. A small map fits its width as the Go map did (a little wider is drawn a little
    /// smaller, else 1:1 and it scrolls down); a large one is shown whole, centred.
    pub(crate) fn fit(&mut self, l: &Layout) {
        let (w, h) = self.wh();
        let b = l.bounds;
        self.fitted = true;
        self.flight = None;
        if l.mode == Mode::Classic {
            // The Go map fitted its width when a little wider (down to 84%) and scrolled the page otherwise; with a
            // camera it fits the whole drawing down to 70%, where every word on a card is still readable, then pans.
            let r = (w / b.w.max(1.0) as f32).min(h / b.h.max(1.0) as f32);
            self.zoom = r.clamp(0.7, 1.0);
            self.pan = (0.0, 0.0);
            self.centre_small(b);
            return;
        }
        // The folded tile sits at the bottom left: the drawing fits above it.
        let h = if l.tile.is_some() {
            (h - super::tile::TILE_H - 16.0).max(h * 0.5)
        } else {
            h
        };
        let z = (w / b.w as f32).min(h / b.h as f32).min(1.0);
        self.zoom = if z >= 0.84 {
            z
        } else {
            (z * 0.97).max(MIN_ZOOM)
        };
        let (vw, vh) = (w / self.zoom, h / self.zoom);
        self.pan = (
            ((b.w as f32 - vw) / 2.0).min(0.0),
            ((b.h as f32 - vh) / 2.0).min(0.0),
        );
    }

    /// A small map narrower than the viewport sits in its middle, not against the left.
    fn centre_small(&mut self, b: Rect) {
        let (w, _) = self.wh();
        let drawn = b.w as f32 * self.zoom;
        if drawn < w {
            self.pan.0 = -((w - drawn) / 2.0) / self.zoom;
        }
    }

    /// Zooms by `factor` keeping the map point under (sx, sy) (viewport pixels) where it is.
    pub(crate) fn zoom_at(&mut self, factor: f32, sx: f32, sy: f32, b: Rect) {
        let p = self.to_map(sx, sy);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan = (p.x as f32 - sx / self.zoom, p.y as f32 - sy / self.zoom);
        self.clamp(b);
    }

    /// Zooms about the middle of the viewport.
    pub(crate) fn zoom_centre(&mut self, factor: f32, b: Rect) {
        let (w, h) = self.wh();
        self.zoom_at(factor, w / 2.0, h / 2.0, b);
    }

    /// Moves by (dx, dy) viewport pixels.
    pub(crate) fn pan_by(&mut self, dx: f32, dy: f32, b: Rect) {
        self.pan = (self.pan.0 + dx / self.zoom, self.pan.1 + dy / self.zoom);
        self.clamp(b);
    }

    /// Centres the map point (x, y).
    pub(crate) fn centre_on(&mut self, x: f64, y: f64, b: Rect) {
        let (w, h) = self.wh();
        self.pan = (
            x as f32 - w / self.zoom / 2.0,
            y as f32 - h / self.zoom / 2.0,
        );
        self.clamp(b);
    }

    /// Keeps some of the drawing in view.
    pub(crate) fn clamp(&mut self, b: Rect) {
        let (w, h) = self.wh();
        let (vw, vh) = (w / self.zoom, h / self.zoom);
        self.pan.0 = self
            .pan
            .0
            .clamp(-vw * 0.6, (b.w as f32 - vw * 0.4).max(-vw * 0.6));
        self.pan.1 = self
            .pan
            .1
            .clamp(-vh * 0.6, (b.h as f32 - vh * 0.4).max(-vh * 0.6));
    }

    /// Brings the card into view if it is not, moving as little as a centring does.
    pub(crate) fn reveal(&mut self, l: &Layout, id: CardId) {
        let Some(c) = l.card(id).filter(|_| !l.folded(id)) else {
            return;
        };
        let (v, r) = (self.view(), c.rect);
        if r.x < v.x || r.y < v.y || r.right() > v.right() || r.bottom() > v.bottom() {
            let ctr = r.center();
            self.centre_on(ctr.x, ctr.y, l.bounds);
        }
    }
}

/// The nearest card the arrow points at (flow.tsx `nextCard`), over every card; from nothing, the first in view.
pub(crate) fn next_card(
    l: &Layout,
    cam: &Camera,
    from: Option<CardId>,
    dx: f64,
    dy: f64,
) -> Option<CardId> {
    let Some((from, at)) = from.and_then(|f| l.card(f).map(|c| (f, c.rect.center()))) else {
        return l.visible(cam.view()).cards.into_iter().min();
    };
    let mut best: Option<(f64, CardId)> = None;
    for (i, c) in l.cards.iter().enumerate() {
        let id = CardId(u32::try_from(i).unwrap_or(u32::MAX));
        if id == from || l.folded(id) {
            continue;
        }
        let (ddx, ddy) = (c.rect.center().x - at.x, c.rect.center().y - at.y);
        let along = ddx * dx + ddy * dy;
        let across = (ddx * dy - ddy * dx).abs();
        if along <= 10.0 {
            continue;
        }
        let s = along + 2.0 * across;
        if best.is_none_or(|b| s < b.0) {
            best = Some((s, id));
        }
    }
    best.map(|b| b.1)
}

#[cfg(test)]
mod tests {
    use super::{Camera, MAX_ZOOM, next_card};
    use doze_map::{Options, layout, synth};

    fn cam(w: f32, h: f32) -> Camera {
        Camera {
            size: (w, h),
            ..Camera::default()
        }
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let l = layout(&synth::stack(400, 1000, 3), &Options::default());
        let mut c = cam(1200.0, 800.0);
        c.fit(&l);
        let before = c.to_map(300.0, 200.0);
        c.zoom_at(1.5, 300.0, 200.0, l.bounds);
        let after = c.to_map(300.0, 200.0);
        assert!((before.x - after.x).abs() < 0.5 && (before.y - after.y).abs() < 0.5);
        c.zoom_at(1000.0, 0.0, 0.0, l.bounds);
        assert!((c.zoom - MAX_ZOOM).abs() < f32::EPSILON);
    }

    #[test]
    fn a_large_map_fits_whole_and_arrows_reach_a_card() {
        let l = layout(&synth::stack(2000, 5000, 7), &Options::default());
        let mut c = cam(1800.0, 800.0);
        c.fit(&l);
        assert!(c.zoom < 0.2, "{}", c.zoom);
        let v = c.view();
        assert!(v.w >= l.bounds.w - 1.0 || v.h >= l.bounds.h - 1.0);
        let first = next_card(&l, &c, None, 1.0, 0.0);
        assert!(first.is_some());
        assert!(next_card(&l, &c, first, 1.0, 0.0).is_some());
    }

    #[test]
    fn a_small_map_fits_its_width() {
        let l = layout(&synth::stack(10, 25, 7), &Options::default());
        let mut c = cam(l.bounds.w as f32 * 0.9, 2000.0);
        c.fit(&l);
        assert!((c.zoom - 0.9).abs() < 0.01, "{}", c.zoom);
    }
}
