//! The Map's fades, timed as the Go map's CSS has them (`shell.css`): a selection dims the rest and lights its own over
//! 200 ms on CSS's `ease` (`.map .node { transition: opacity .2s }`, `.map .edge { transition: stroke .2s }`), and the
//! card under the pointer lights over 120 ms easing out (`.node { transition: border-color 120ms ease-out }`), its
//! calls fading in as the last card's fade out. A focus view (or a new layout) fades in over the app's `normal` token.
//! Nothing pops: every change of the selection or the pointer's card blends from what was drawn. With motion reduced
//! there is no blending at all and no frame is asked for.
//!
//! The renderer asks [`Fades::lit`] and [`Fades::hover`] for how far along each blend is, and redraws while one runs.

use std::rc::Rc;
use std::time::{Duration, Instant};

use doze_map::CardId;

/// The Go map's dimming: `opacity .2s` on CSS's default `ease`.
pub(crate) const DIM: Duration = Duration::from_millis(200);
/// The Go map's hover: `border-color 120ms ease-out`.
pub(crate) const HOVER: Duration = Duration::from_millis(120);
/// A faded card's opacity (`.map .faded { opacity: .32 }`).
pub(crate) const FADED: f32 = 0.32;

/// A cubic Bézier easing curve through (0,0), (x1,y1), (x2,y2), (1,1), as CSS defines one: `y` for a given `x`.
pub(crate) fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let at = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    // Bisection on x(t): monotonic for CSS's curves, and exact enough in 20 steps.
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..20 {
        let mid = f32::midpoint(lo, hi);
        if at(x1, x2, mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    at(y1, y2, f32::midpoint(lo, hi))
}

/// CSS's `ease`.
pub(crate) fn ease(x: f32) -> f32 {
    bezier(0.25, 0.1, 0.25, 1.0, x)
}

/// CSS's `ease-out`.
pub(crate) fn ease_out(x: f32) -> f32 {
    bezier(0.0, 0.0, 0.58, 1.0, x)
}

/// `a` blended toward `b` by `p`, alpha too.
pub(crate) fn blend(a: gpui::Hsla, b: gpui::Hsla, p: f32) -> gpui::Hsla {
    if p >= 1.0 {
        return b;
    }
    let (x, y) = (gpui::Rgba::from(a), gpui::Rgba::from(b));
    let l = |u: f32, v: f32| u + (v - u) * p;
    gpui::Rgba {
        r: l(x.r, y.r),
        g: l(x.g, y.g),
        b: l(x.b, y.b),
        a: l(x.a, y.a),
    }
    .into()
}

/// What is blending, and from what.
#[derive(Debug, Clone, Default)]
pub(crate) struct Fades {
    /// The selection as last drawn, and what it kept lit.
    last_lit: Option<CardId>,
    last_on: Option<Rc<Vec<bool>>>,
    /// The selection and lit set being blended away from, and since when.
    pub(crate) lit_from: Option<CardId>,
    pub(crate) on_from: Option<Rc<Vec<bool>>>,
    lit_since: Option<Instant>,
    /// The pointer's card as last drawn; the one being blended away from, and since when.
    last_hover: Option<CardId>,
    pub(crate) hover_from: Option<CardId>,
    hover_since: Option<Instant>,
    /// When the drawn layout last changed (a focus view opened or closed).
    shown_since: Option<Instant>,
    /// Durations: zero with motion reduced.
    dim: Duration,
    hover: Duration,
    show: Duration,
    /// Held at this share of the way (shots and tests), instead of the clock.
    pub(crate) held: Option<f32>,
}

fn share(since: Option<Instant>, dur: Duration, now: Instant, held: Option<f32>) -> f32 {
    if let Some(h) = held {
        return h.clamp(0.0, 1.0);
    }
    match since {
        Some(s) if !dur.is_zero() => {
            (now.saturating_duration_since(s).as_secs_f32() / dur.as_secs_f32()).min(1.0)
        }
        _ => 1.0,
    }
}

impl Fades {
    /// Notes this frame's selection, lit set and pointer's card, starting a blend for whatever changed; `durations`
    /// are the dim, hover and show times in force (zero when motion is reduced).
    pub(crate) fn note(
        &mut self,
        lit: Option<CardId>,
        on: Option<Rc<Vec<bool>>>,
        hover: Option<CardId>,
        durations: (Duration, Duration, Duration),
        now: Instant,
    ) {
        (self.dim, self.hover, self.show) = durations;
        if lit != self.last_lit
            || on.as_ref().map(|v| v.len()) != self.last_on.as_ref().map(|v| v.len())
        {
            self.lit_from = self.last_lit;
            self.on_from = self.last_on.take();
            self.lit_since = Some(now);
            self.last_lit = lit;
        }
        self.last_on = on;
        if hover != self.last_hover {
            self.hover_from = self.last_hover;
            self.hover_since = Some(now);
            self.last_hover = hover;
        }
    }

    /// A new layout is drawn: the old cards' ids mean nothing in it, so nothing blends from them; it fades in.
    pub(crate) fn shown(&mut self, lit: Option<CardId>, hover: Option<CardId>) {
        self.last_lit = lit;
        self.last_on = None;
        self.lit_from = None;
        self.on_from = None;
        self.lit_since = None;
        self.last_hover = hover;
        self.hover_from = None;
        self.hover_since = None;
        self.shown_since = Some(Instant::now());
    }

    /// How far the selection's blend is, eased (1 when none runs).
    pub(crate) fn lit(&self, now: Instant) -> f32 {
        ease(share(self.lit_since, self.dim, now, self.held))
    }

    /// How far the pointer's blend is, eased.
    pub(crate) fn hover(&self, now: Instant) -> f32 {
        ease_out(share(self.hover_since, self.hover, now, self.held))
    }

    /// How far a new layout's fade-in is, eased.
    pub(crate) fn shown_in(&self, now: Instant) -> f32 {
        // A held frame (a shot) is of a hover or a selection: the layout is in.
        ease_out(share(
            self.shown_since,
            self.show,
            now,
            self.held.map(|_| 1.0),
        ))
    }

    /// Whether a blend still runs (the renderer asks for another frame).
    pub(crate) fn running(&self, now: Instant) -> bool {
        self.held.is_none()
            && (share(self.lit_since, self.dim, now, None) < 1.0
                || share(self.hover_since, self.hover, now, None) < 1.0
                || share(self.shown_since, self.show, now, None) < 1.0)
    }

    /// A card's opacity under the selection: 1 lit (or no selection), [`FADED`] faded, blended from before.
    pub(crate) fn opacity(&self, on: Option<&[bool]>, i: usize, p: f32) -> f32 {
        let of = |on: Option<&[bool]>| match on {
            Some(on) if !on.get(i).copied().unwrap_or(false) => FADED,
            _ => 1.0,
        };
        let from = of(self.on_from.as_deref().map(Vec::as_slice));
        from + (of(on) - from) * p
    }

    /// How hot (ringed, its calls drawn) a card is under the pointer: the new card blending in, the last out.
    pub(crate) fn heat(&self, id: CardId, hover: Option<CardId>, p: f32) -> f32 {
        if hover == Some(id) {
            p
        } else if self.hover_from == Some(id) {
            1.0 - p
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use super::{DIM, FADED, Fades, HOVER, ease, ease_out};
    use doze_map::CardId;

    #[test]
    fn the_curves_are_css() {
        assert!(ease(0.0).abs() < 1e-3 && (ease(1.0) - 1.0).abs() < 1e-3);
        // CSS ease at half way is about 0.80; ease-out about 0.69.
        assert!((ease(0.5) - 0.802).abs() < 0.01, "{}", ease(0.5));
        assert!((ease_out(0.5) - 0.687).abs() < 0.01, "{}", ease_out(0.5));
    }

    #[test]
    fn a_selection_blends_from_what_was_drawn() {
        let mut f = Fades::default();
        let t0 = Instant::now();
        let d = (DIM, HOVER, DIM);
        f.note(None, None, None, d, t0);
        let on = Rc::new(vec![true, false]);
        f.note(Some(CardId(0)), Some(on.clone()), None, d, t0);
        // At the start nothing has dimmed; at the end card 1 is faded.
        assert!((f.opacity(Some(&on), 1, 0.0) - 1.0).abs() < 1e-6);
        assert!((f.opacity(Some(&on), 1, 1.0) - FADED).abs() < 1e-6);
        assert!(f.running(t0 + Duration::from_millis(100)));
        assert!(!f.running(t0 + Duration::from_millis(250)));
        // The pointer: the new card heats as the last cools.
        f.note(Some(CardId(0)), Some(on.clone()), Some(CardId(1)), d, t0);
        f.note(Some(CardId(0)), Some(on), Some(CardId(0)), d, t0);
        assert!((f.heat(CardId(0), Some(CardId(0)), 0.25) - 0.25).abs() < 1e-6);
        assert!((f.heat(CardId(1), Some(CardId(0)), 0.25) - 0.75).abs() < 1e-6);
        // Reduced motion: no time, so nothing runs.
        let zero = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
        f.note(None, None, None, zero, t0);
        assert!(!f.running(t0) && (f.lit(t0) - 1.0).abs() < 1e-6);
    }
}
