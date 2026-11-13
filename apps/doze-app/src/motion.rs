//! The page's own motion, timed as the Go page's CSS has it (`shell.css`): `settle` on a banner or a card as it mounts
//! (240 ms, from 6 px above, `cubic-bezier(.2,.7,.3,1)`), and colour transitions on hover and selection (`120ms
//! ease-out` on background, border and colour). Reduced motion turns off the keyframe animations, as Go's
//! `prefers-reduced-motion` rule does, and leaves the transitions.
//!
//! A transition here is remembered per element id: the colour it was going to and when that changed. While one
//! runs, the element asks for the next frame; when none does, nothing is scheduled.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gpui::{Animation, AnimationExt as _, AnyElement, Hsla, IntoElement, Styled, px};

/// Go's `settle`, `rise` and `grow` curve: `cubic-bezier(.2, .7, .3, 1)`.
pub(crate) fn curve(x: f32) -> f32 {
    crate::map::fade::bezier(0.2, 0.7, 0.3, 1.0, x)
}

/// CSS's `ease-out`: `cubic-bezier(0, 0, .58, 1)`.
pub(crate) fn ease_out(x: f32) -> f32 {
    crate::map::fade::bezier(0.0, 0.0, 0.58, 1.0, x)
}

/// Go's colour transitions (`.btn`, `.tile`, `.lrow`, `.si`, `.bell`, `.stab`): 120 ms ease-out.
pub(crate) const COLOUR: Duration = Duration::from_millis(120);

static REDUCED: AtomicBool = AtomicBool::new(false);

/// Says, once a frame, whether the system asks to reduce motion.
pub(crate) fn set_reduced(reduced: bool) {
    REDUCED.store(reduced, Ordering::Relaxed);
}

/// Whether keyframe animations are off.
pub(crate) fn reduced() -> bool {
    REDUCED.load(Ordering::Relaxed)
}

/// Go's `settle`: fades `el` in from `6 px` above over `ms`, the first frame it shows (again each time it mounts).
pub(crate) fn settle<E>(id: &'static str, el: E, ms: u64) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    if reduced() {
        return el.into_any_element();
    }
    el.relative()
        .with_animation(
            id,
            Animation::new(Duration::from_millis(ms)).with_easing(curve),
            |d, p| d.opacity(p).top(px(-6.0 * (1.0 - p))),
        )
        .into_any_element()
}

/// Go's keyframe entrances, as `shell.css` times them (all on [`curve`]): `settle` 240 ms (banners, the away strip,
/// the source card) and 160 ms (the bell's panel, popovers); `rise` 220 ms (toasts); `grow` 180 ms (palette, dialogs,
/// help).
pub(crate) fn go_anim(ms: u64) -> Animation {
    Animation::new(Duration::from_millis(ms)).with_easing(curve)
}

/// Go's `fade` on a scrim: 160 ms, `ease-out`.
pub(crate) fn scrim_anim() -> Animation {
    Animation::new(Duration::from_millis(160)).with_easing(ease_out)
}

/// A colour on its way somewhere: from, to, since.
#[derive(Clone, Copy)]
struct Blend {
    from: Hsla,
    to: Hsla,
    at: Instant,
}

thread_local! {
    static BLENDS: RefCell<HashMap<String, Blend>> = RefCell::new(HashMap::new());
}

/// The colour the element `id` shows now on its way to `to` (a 120 ms ease-out from where it was when `to` last
/// changed), and whether it is still moving (the caller asks for another frame while it is).
pub(crate) fn colour(id: &str, to: Hsla) -> (Hsla, bool) {
    BLENDS.with(|b| {
        let mut b = b.borrow_mut();
        let now = Instant::now();
        let e = b.entry(id.to_owned()).or_insert(Blend {
            from: to,
            to,
            at: now.checked_sub(COLOUR).unwrap_or(now),
        });
        if e.to != to {
            let (shown, _) = at(e, now);
            *e = Blend {
                from: shown,
                to,
                at: now,
            };
        }
        at(e, now)
    })
}

fn at(b: &Blend, now: Instant) -> (Hsla, bool) {
    let x = now.duration_since(b.at).as_secs_f32() / COLOUR.as_secs_f32();
    if x >= 1.0 {
        return (b.to, false);
    }
    (mix(b.from, b.to, ease_out(x)), true)
}

/// `a` to `b` by `p`, in premultiplied RGB as a browser interpolates a colour transition (so a fade from
/// transparent is a fade of the colour, never through black).
fn mix(a: Hsla, b: Hsla, p: f32) -> Hsla {
    let (a, b) = (a.to_rgb(), b.to_rgb());
    let l = |x: f32, y: f32| x + (y - x) * p;
    let alpha = l(a.a, b.a);
    if alpha <= f32::EPSILON {
        return gpui::transparent_black();
    }
    gpui::Rgba {
        r: l(a.r * a.a, b.r * b.a) / alpha,
        g: l(a.g * a.a, b.g * b.a) / alpha,
        b: l(a.b * a.a, b.b * b.a) / alpha,
        a: alpha,
    }
    .into()
}

/// Go's switch knob: `transition: left .12s ease-out`.
pub(crate) const KNOB: Duration = Duration::from_millis(120);

thread_local! {
    static SLIDES: RefCell<HashMap<String, (f32, f32, Instant)>> = RefCell::new(HashMap::new());
}

/// Where the knob `id` is now on its way to `to` (120 ms ease-out from where it was when `to` changed; the first
/// time an id is seen it is already there), and whether it is still moving.
pub(crate) fn slide(id: &str, to: f32) -> (f32, bool) {
    SLIDES.with(|s| {
        let mut s = s.borrow_mut();
        let now = Instant::now();
        let e = s
            .entry(id.to_owned())
            .or_insert((to, to, now.checked_sub(KNOB).unwrap_or(now)));
        let at = |e: &(f32, f32, Instant)| {
            let x = now.duration_since(e.2).as_secs_f32() / KNOB.as_secs_f32();
            if x >= 1.0 {
                (e.1, false)
            } else {
                (e.0 + (e.1 - e.0) * ease_out(x), true)
            }
        };
        if (e.1 - to).abs() > f32::EPSILON {
            let (shown, _) = at(e);
            *e = (shown, to, now);
        }
        let (x, moving) = at(e);
        #[cfg(feature = "fixtures")]
        if moving || (x - to).abs() < f32::EPSILON && now.duration_since(e.2) < KNOB * 2 {
            crate::driver::note("knob", &format!("{id} left={x:.1}"));
        }
        (x, moving)
    })
}

thread_local! {
    /// The elements under the pointer, by id, as their `on_hover` last said.
    static HOVERED: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
}

/// Whether the element `id` is under the pointer.
pub(crate) fn hovered(id: &str) -> bool {
    HOVERED.with(|h| h.borrow().contains(id))
}

/// Keeps track of whether the element `id` is under the pointer, for [`colour`] to ease towards its hover colours.
pub(crate) fn track<E: gpui::StatefulInteractiveElement>(el: E, id: &str) -> E {
    let id = id.to_owned();
    el.on_hover(move |over, window, _| {
        let changed = HOVERED.with(|h| {
            let mut h = h.borrow_mut();
            if *over {
                h.insert(id.clone())
            } else {
                h.remove(&id)
            }
        });
        if changed {
            window.refresh();
        }
    })
}

/// A canvas that asks for the next frame while `moving`: put beside an element whose colour is blending.
pub(crate) fn tick(moving: bool) -> impl IntoElement {
    gpui::canvas(
        move |_, window, _| {
            if moving {
                window.request_animation_frame();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_0()
}

#[cfg(test)]
mod tests {
    use super::{colour, curve};

    #[test]
    fn a_colour_eases_to_where_it_is_going_and_then_stops() {
        let (red, blue) = (gpui::red(), gpui::blue());
        assert_eq!(colour("t", red), (red, false));
        let (now, moving) = colour("t", blue);
        assert!(moving);
        assert_ne!(now, blue);
        std::thread::sleep(super::COLOUR);
        assert_eq!(colour("t", blue), (blue, false));
        assert!((curve(1.0) - 1.0).abs() < 1e-3 && curve(0.0).abs() < 1e-3);
    }
}
