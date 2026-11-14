//! The Map's cards up close, as flow.tsx and `.map` in shell.css drew them, scaled by the zoom (CSS `zoom`: every
//! length together) and placed in the viewport; only the cards in view are built. The captions, and between far and
//! close, the cards' names.

use doze_map::{Card, CardId, Kind, Role};
use gpui::{AnyElement, BoxShadow, IntoElement, SharedString, Styled, div, point, prelude::*, px};

use super::MapView;
use crate::instance::ui::state_mark;
use crate::model::Row;
use crate::outward::Probe as _;
use crate::theme::{Tokens, alpha, mix, ty};
use crate::widgets::{MONO_FAMILY, icon};

/// Lengths scaled by the zoom.
#[derive(Clone, Copy)]
pub(crate) struct Z(pub(crate) f32);

impl Z {
    pub(crate) fn p(self, v: f32) -> gpui::Pixels {
        px(v * self.0)
    }
}

/// The state mark every screen draws for a look ([`crate::instance::ui::state_mark`]), at the card's zoom.
pub(crate) fn state_dot(t: &Tokens, size: gpui::Pixels, r: Option<&Row>) -> AnyElement {
    state_mark(
        t,
        r.map(|r| r.look),
        r.is_some_and(|r| r.trouble),
        f32::from(size),
    )
}

fn badge(t: &Tokens, z: Z, text: String) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(z.p(14.))
        .px(z.p(4.))
        .rounded(z.p(4.))
        .border_1()
        .border_color(t.line)
        .font_family(MONO_FAMILY)
        .text_size(z.p(ty::MICRO))
        .font_weight(ty::SEMIBOLD)
        .text_color(t.dim)
        .child(SharedString::from(text))
}

/// The top row, as Go's `.top`: dot, name, then on an app's card its language and pin, and the memory at the right
/// while it is awake (nothing otherwise: the mark already says the state). A band card has no language or pin.
fn top_row(t: &Tokens, z: Z, c: &Card, r: Option<&Row>, node: bool) -> impl IntoElement {
    let name = div()
        .min_w_0()
        .text_size(z.p(ty::MD))
        .line_height(z.p(ty::LH))
        .font_weight(ty::HEADING)
        .text_color(t.bright)
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .child(SharedString::from(c.name.clone()));
    let dot = state_dot(t, z.p(8.), r);
    let mut row = div()
        .flex()
        .items_center()
        .gap(z.p(7.))
        .w_full()
        .min_w_0()
        .child(dot)
        .child(name);
    if node && let Some(l) = c.lang {
        row = row.child(badge(t, z, l.badge().to_string()));
    }
    // A collapsed group's card (only a large map collapses one) says how many it stands for.
    if c.members > 1 {
        row = row.child(badge(t, z, format!("×{}", c.members)));
    }
    if node && r.is_some_and(|r| r.pinned) {
        row = row.child(
            div()
                .flex_none()
                .child(icon("icons/pin.svg", 13. * z.0, t.accent)),
        );
    }
    if let Some(text) = super::state::mem(r) {
        row = row.child(
            div()
                .ml_auto()
                .flex_none()
                .text_size(z.p(ty::SM))
                .text_color(t.dim)
                .font_features(ty::tabular())
                .child(SharedString::from(text)),
        );
    }
    row
}

fn frame(t: &Tokens, z: Z, c: &Card, at: (f32, f32), radius: f32) -> gpui::Div {
    div()
        .absolute()
        .left(px(at.0))
        .top(px(at.1))
        .w(z.p(c.rect.w as f32))
        .h(z.p(c.rect.h as f32))
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(z.p(radius))
        .border_1()
        .border_color(t.line)
        .bg(t.panel)
        .text_color(t.text)
        .line_height(z.p(ty::LH))
}

/// The accent ring, at `a` of its strength (it fades in and out with the hover and the selection).
fn ring(t: &Tokens, z: Z, strong: bool, a: f32) -> Vec<BoxShadow> {
    // Opaque, blended from the well: it fades in without tinting the card it rings.
    let mut v = vec![BoxShadow {
        color: if a >= 1.0 {
            t.accent
        } else {
            mix(t.accent, t.well, a * 100.0)
        },
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(1.),
        inset: false,
    }];
    if strong {
        v.push(BoxShadow {
            color: alpha(t.accent, 0.16 * a),
            offset: point(px(0.), z.p(8.)),
            blur_radius: z.p(24.),
            spread_radius: px(0.),
            inset: false,
        });
    }
    v
}

/// What a card's state says: how hot its ring is (selected or hovered, 0 to 1 as it fades), whether it is the selection,
/// and its opacity under a selection (1, or faded to .32, blending; `super::fade`).
#[derive(Clone, Copy)]
struct Mark {
    hot: f32,
    sel: bool,
    opacity: f32,
}

fn chips(t: &Tokens, z: Z, m: &MapView, c: &Card, mark: Mark) -> gpui::Div {
    let r = m.rows.get(&c.name);
    let chip = |text: &str| {
        let d = div()
            .flex()
            .flex_none()
            .items_center()
            .h(z.p(20.))
            .px(z.p(7.))
            .rounded(z.p(10.))
            .text_size(z.p(ty::XS_UP));
        let d = if mark.sel {
            d.bg(t.accent).text_color(t.on_accent)
        } else {
            d.bg(t.sel).text_color(t.text)
        };
        d.child(SharedString::from(text.to_string()))
    };
    let mut row = div()
        .flex()
        .items_center()
        .gap(z.p(5.))
        .w_full()
        .overflow_hidden()
        .whitespace_nowrap();
    if c.quiet {
        let (word, color) = match (c.kind, r) {
            (Kind::Task, Some(r)) => (r.look.word().to_string(), t.tone(r.look.tone())),
            _ => ("no shared services".to_string(), t.dim),
        };
        row = row.child(
            div()
                .flex_none()
                .text_size(z.p(ty::XS_UP))
                .text_color(color)
                .child(SharedString::from(word)),
        );
    }
    for s in &c.chips {
        row = row.child(chip(s));
    }
    if c.more > 0 {
        row = row.child(
            div()
                .flex_none()
                .text_size(z.p(ty::XS_UP))
                .text_color(t.dim)
                .child(SharedString::from(format!("+{}", c.more))),
        );
    }
    if c.fold {
        let f = m.folds.get(&c.name);
        let (glyph, color) = f.map_or(("✓", t.good), |f| (f.glyph, t.tone(f.tone)));
        let title = f.map(|f| SharedString::from(f.title.clone()));
        row = row.child(
            div()
                .id(SharedString::from(format!("fold-{}", c.name)))
                .when_some(title, |d, title| {
                    d.tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(title.clone()).build(window, cx)
                    })
                })
                .ml_auto()
                .flex_none()
                .text_size(z.p(ty::XS_UP))
                .text_color(color)
                .child(glyph),
        );
    }
    row
}

/// A small map's card under the pointer: a plain border turns `line2` over 120 ms (Go `.node:hover`); a red or
/// selected one keeps its own.
fn framed_border(t: &Tokens, m: &MapView, c: &Card, border: gpui::Hsla) -> (gpui::Hsla, bool) {
    let over = m
        .framed
        .is_some_and(|id| m.layout.card(id).is_some_and(|f| f.name == c.name));
    let to = if over && border == t.line {
        t.line2
    } else {
        border
    };
    crate::motion::colour(&format!("card/{}", c.name), to)
}

fn node_card(t: &Tokens, z: Z, m: &MapView, c: &Card, at: (f32, f32), mark: Mark) -> AnyElement {
    let r = m.rows.get(&c.name);
    let task = c.kind == Kind::Task;
    let mut f = frame(t, z, c, at, if task { 8. } else { 11. })
        .gap(z.p(7.))
        .px(z.p(13.))
        .py(z.p(11.));
    let st = super::state::frame(t, r, task);
    let (border, moving) = framed_border(t, m, c, st.border);
    f = f
        .border_color(border)
        .bg(st.fill)
        .child(crate::motion::tick(moving))
        .probe(format!("card-{}", c.name), c.name.clone());
    if st.dashed {
        f = f.border_dashed();
    }
    if mark.hot > 0.0 {
        let was = st.border;
        f = f.border_color(mix(t.accent, was, mark.hot * 100.0));
        // The ring fades with the border: an opaque outline blended from the well to the accent.
        f = f.shadow(ring(t, z, mark.sel, mark.hot));
    }
    f.when(mark.opacity < 1.0, |d| d.opacity(mark.opacity))
        .child(top_row(t, z, c, r, true))
        .child(chips(t, z, m, c, mark))
        .into_any_element()
}

fn shared_card(t: &Tokens, z: Z, m: &MapView, c: &Card, at: (f32, f32), mark: Mark) -> AnyElement {
    let r = m.rows.get(&c.name);
    let mut f = frame(t, z, c, at, 11.)
        .gap(z.p(3.))
        .px(z.p(13.))
        .py(z.p(10.));
    let st = super::state::frame(t, r, false);
    let (border, moving) = framed_border(t, m, c, st.border);
    f = f
        .border_color(border)
        .bg(st.fill)
        .child(crate::motion::tick(moving))
        .probe(format!("card-{}", c.name), c.name.clone());
    if st.dashed {
        f = f.border_dashed();
    }
    if mark.hot > 0.0 {
        let was = st.border;
        f = f.border_color(mix(t.accent, was, mark.hot * 100.0));
        f = f.shadow(ring(t, z, false, mark.hot));
    }
    let line = || {
        div()
            .w_full()
            .text_size(z.p(ty::SM))
            .line_height(z.p(ty::LH))
            .text_color(t.dim)
            .whitespace_nowrap()
            .overflow_hidden()
    };
    let what = match (c.kind, r.map(|r| r.engine.clone())) {
        (Kind::Service, _) => "brought in by you".to_string(),
        (_, Some(e)) if !e.is_empty() => e,
        _ => "module".to_string(),
    };
    let users = c.uses.len();
    let mut used = div()
        .flex()
        .child(
            div()
                .font_weight(ty::SEMIBOLD)
                .text_color(t.text)
                .child(SharedString::from(match users {
                    0 => "nothing uses it".to_string(),
                    n => format!("used by {n}"),
                })),
        );
    if users == 1
        && let Some(u) = c.uses.first().and_then(|u| m.layout.card(*u))
    {
        used = used.child(SharedString::from(format!(" · {}", u.name)));
    }
    f.when(mark.opacity < 1.0, |d| d.opacity(mark.opacity))
        .child(top_row(t, z, c, r, false))
        .child(line().child(SharedString::from(what)))
        .child(line().child(used))
        .into_any_element()
}

/// The cards in view.
pub(crate) fn cards(
    t: &Tokens,
    m: &MapView,
    vis: &[CardId],
    on: Option<&[bool]>,
) -> Vec<AnyElement> {
    let z = Z(m.cam.zoom);
    let now = std::time::Instant::now();
    let (pl, ph) = (m.fades.lit(now), m.fades.hover(now));
    vis.iter()
        .filter_map(|&id| {
            let c = m.layout.card(id)?;
            let at = m.cam.to_screen(c.rect.x, c.rect.y);
            let lit_here = on.map(|on| on.get(id.ix()).copied().unwrap_or(false));
            let sel = m.lit == Some(id);
            let mark = Mark {
                hot: if sel {
                    pl
                } else {
                    m.fades.heat(id, m.hover, ph)
                },
                sel,
                opacity: m.fades.opacity(on, id.ix(), pl),
            };
            Some(if c.role == Role::Shared {
                // A shared card lights when the selection uses it.
                let used = lit_here == Some(true)
                    && m.lit
                        .and_then(|l| m.layout.card(l))
                        .is_some_and(|lc| lc.role != Role::Shared);
                shared_card(
                    t,
                    z,
                    m,
                    c,
                    at,
                    Mark {
                        hot: if used { mark.hot.max(pl) } else { mark.hot },
                        ..mark
                    },
                )
            } else {
                node_card(t, z, m, c, at, mark)
            })
        })
        .collect()
}
