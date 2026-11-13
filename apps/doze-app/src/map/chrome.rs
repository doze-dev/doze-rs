//! Around the drawing: the toolbar strip above it (zoom, find, overview, what the map holds, the legend), the
//! minimap, the find box and the selected card's detail. The toolbar is a strip of its own, so it never covers a card
//! and fitting needs to reserve nothing for it; the minimap and the detail card each take the corner the card in hand
//! is not in.

use doze_map::{Mode, Rect};
use gpui::{
    Animation, AnimationExt as _, AnyElement, Context, IntoElement, SharedString, div, prelude::*,
    px,
};

use super::MapView;
use crate::actions::{self, FindCard, FitMap, ToggleMinimap};
use crate::theme::{Motion, Tokens, ty};
use crate::widgets::{icon, kbd_plain};

/// The toolbar strip's height.
pub(crate) const STRIP: f32 = 40.;

fn tool(t: &Tokens, id: &'static str, label: &str, on: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .h(px(26.))
        .min_w(px(28.))
        .px(px(8.))
        .rounded(px(7.))
        .text_size(px(ty::SM))
        .text_color(if on { t.bright } else { t.text })
        .bg(if on { t.sel } else { t.panel })
        .border_1()
        .border_color(t.line)
        .cursor_pointer()
        .hover(|s| s.bg(t.sel))
        .child(SharedString::from(label.to_string()))
}

/// What the map holds, and the zoom: `1476 cards · 5013 calls · 5% · 258 bundles`.
fn stats(m: &MapView) -> String {
    let l = &m.layout;
    let calls: u32 = l.edges.iter().map(|e| e.count.max(1)).sum();
    let mut stats = format!(
        "{} cards · {} calls · {:.0}%",
        l.cards.len(),
        calls,
        m.cam.zoom * 100.0
    );
    if super::paint::bundled(m) {
        stats.push_str(&format!(" · {} bundles", m.overview.bundles.len()));
    }
    stats
}

/// The strip: zoom out, in, fit, 1:1, find, overview; what the map holds; the legend or the hint.
pub(crate) fn strip(t: &Tokens, m: &MapView, cx: &mut Context<MapView>) -> AnyElement {
    let stats = stats(m);
    let find_key = actions::TABLE
        .iter()
        .find(|s| s.label == "Find a card")
        .and_then(actions::chip)
        .unwrap_or_default();
    let mut bar = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(STRIP))
        .px(px(10.))
        .border_b_1()
        .border_color(t.line)
        .child(
            tool(t, "z-out", "−", false)
                .on_click(cx.listener(|this, _, _, cx| this.zoom_by(0.8, cx))),
        )
        .child(
            tool(t, "z-in", "+", false)
                .on_click(cx.listener(|this, _, _, cx| this.zoom_by(1.25, cx))),
        )
        .child(
            tool(t, "z-fit", "Fit", false)
                .on_click(cx.listener(|this, _, window, cx| this.on_fit(&FitMap, window, cx))),
        )
        .child(
            tool(t, "z-one", "1:1", false).on_click(cx.listener(|this, _, _, cx| {
                let z = this.cam.zoom;
                this.zoom_by(1.0 / z, cx);
            })),
        )
        .child(
            tool(t, "z-find", "Find", m.search.is_some())
                .child(kbd_plain(&find_key, ty::XS, t.dim, ty::MEDIUM))
                .on_click(cx.listener(|this, _, window, cx| this.on_find(&FindCard, window, cx))),
        );
    if m.big() {
        bar = bar.child(minimap_tool(t, m, cx));
    }
    bar = bar.child(
        div()
            .flex_none()
            .ml(px(6.))
            .text_size(px(ty::XS_UP))
            .text_color(t.dim)
            .whitespace_nowrap()
            .child(SharedString::from(stats)),
    );
    // The legend gives way first when the window is narrow.
    bar.child(
        div()
            .flex_1()
            .min_w_0()
            .ml(px(16.))
            .flex()
            .justify_end()
            .overflow_hidden()
            .child(legend(t, m)),
    )
    .into_any_element()
}

/// The overview's switch.
fn minimap_tool(t: &Tokens, m: &MapView, cx: &mut Context<MapView>) -> impl IntoElement {
    let color = if m.minimap { t.bright } else { t.dim };
    tool(t, "z-mini", "", m.minimap)
        .child(icon("icons/minimap.svg", 14., color))
        .on_click(cx.listener(|this, _, window, cx| this.on_minimap(&ToggleMinimap, window, cx)))
}

/// The small map's legend (flow.tsx), or, for a large one, how to see a card's calls.
fn legend(t: &Tokens, m: &MapView) -> AnyElement {
    let base = div()
        .flex()
        .items_center()
        .gap(px(16.))
        .text_size(px(ty::SM))
        .text_color(t.dim)
        .whitespace_nowrap();
    if m.layout.mode != Mode::Classic {
        let hint = if super::paint::bundled(m) {
            "lines are bundled calls between regions · hover a card for its own"
        } else {
            "hover a card to see its calls · click to keep them"
        };
        return base.child(hint).into_any_element();
    }
    base.child(
        div()
            .flex()
            .items_center()
            .child(
                div()
                    .w(px(24.))
                    .h(px(0.))
                    .mr(px(6.))
                    .border_t_2()
                    .border_color(t.accent),
            )
            .child("calls, from references in doze.hcl"),
    )
    .child("chips · what it uses below")
    .child(
        div()
            .flex()
            .child(
                div()
                    .mr(px(4.))
                    .font_weight(ty::BOLD)
                    .text_color(t.good)
                    .child("✓"),
            )
            .child("its setup tasks ran"),
    )
    .child("click a card to draw its lines")
    .into_any_element()
}

/// Which corner of the viewport an overlay of `size` takes: the bottom (or top) right, unless the card in hand is
/// under it, then the left. Coordinates are the viewport's.
pub(crate) fn corner(m: &MapView, size: (f32, f32), top: bool, inset: f32) -> (f32, f32) {
    let (vw, vh) = m.cam.size;
    let y = if top { inset } else { vh - size.1 - inset };
    let right = Rect::new(
        f64::from(vw - size.0 - inset),
        f64::from(y),
        f64::from(size.0),
        f64::from(size.1),
    );
    let covered = m
        .focus_card()
        .and_then(|c| m.layout.card(c))
        .is_some_and(|c| {
            let (x0, y0) = m.cam.to_screen(c.rect.x, c.rect.y);
            let r = Rect::new(
                f64::from(x0),
                f64::from(y0),
                c.rect.w * f64::from(m.cam.zoom),
                c.rect.h * f64::from(m.cam.zoom),
            );
            r.intersects(&right)
        });
    if covered {
        (inset, y)
    } else {
        (vw - size.0 - inset, y)
    }
}

/// The minimap's place and size in viewport pixels, when it shows.
pub(crate) fn minimap_rect(m: &MapView) -> Option<(f32, f32, f32, f32)> {
    if !m.minimap || !m.big() {
        return None;
    }
    let b = m.layout.bounds;
    let scale = (176.0 / b.w as f32).min(112.0 / b.h as f32);
    let size = (b.w as f32 * scale, b.h as f32 * scale);
    let (x, y) = corner(m, (size.0 + 8., size.1 + 8.), false, 12.);
    Some((x + 4., y + 4., size.0, size.1))
}

/// An overlay's entrance: a fade and a few pixels of travel along (dx, dy), gentle and short; nothing at all when
/// motion is reduced.
pub(crate) fn enter(
    el: impl IntoElement + 'static,
    id: impl Into<gpui::ElementId>,
    motion: Motion,
    dx: f32,
    dy: f32,
) -> AnyElement {
    let el = div().child(el);
    if !motion.moves() {
        return el.into_any_element();
    }
    let travel = motion.travel;
    el.with_animation(
        id,
        Animation::new(motion.normal).with_easing(gpui::ease_out_quint()),
        move |d, p| {
            d.opacity(p)
                .relative()
                .left(px(dx * travel * (1.0 - p)))
                .top(px(dy * travel * (1.0 - p)))
        },
    )
    .into_any_element()
}
