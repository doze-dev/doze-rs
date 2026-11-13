//! The Map's cards up close, as flow.tsx and `.map` in shell.css drew them, scaled by the zoom (CSS `zoom`: every
//! length together) and placed in the viewport; only the cards in view are built. The captions, and between far and
//! close, the cards' names.

use doze_display::Kind as WordKind;
use doze_map::{Card, CardId, Kind, Role};
use gpui::{AnyElement, BoxShadow, IntoElement, SharedString, Styled, div, point, prelude::*, px};

use super::MapView;
use crate::model::Row;
use crate::theme::{Tokens, alpha, mix};
use crate::widgets::{MONO_FAMILY, w};

/// Lengths scaled by the zoom.
#[derive(Clone, Copy)]
pub(crate) struct Z(pub(crate) f32);

impl Z {
    pub(crate) fn p(self, v: f32) -> gpui::Pixels {
        px(v * self.0)
    }
}

/// The state mark every screen draws for a look ([`super::state::mark`]).
pub(crate) fn state_dot(t: &Tokens, size: gpui::Pixels, r: Option<&Row>) -> AnyElement {
    super::state::mark(t, r.map(|r| r.look), r.is_some_and(|r| r.trouble), size)
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
        .text_size(z.p(10.))
        .font_weight(w(600.))
        .text_color(t.dim)
        .child(SharedString::from(text))
}

fn top_row(t: &Tokens, z: Z, c: &Card, r: Option<&Row>) -> impl IntoElement {
    let name = div()
        .flex_none()
        .text_size(z.p(14.))
        .line_height(z.p(20.))
        .font_weight(w(650.))
        .text_color(t.bright)
        .whitespace_nowrap()
        .child(SharedString::from(c.name.clone()));
    let dot = state_dot(t, z.p(8.), r);
    let mut row = div()
        .flex()
        .items_center()
        .gap(z.p(7.))
        .w_full()
        .child(dot)
        .child(name);
    if let Some(l) = c.lang {
        row = row.child(badge(t, z, l.badge().to_string()));
    }
    if c.members > 1 {
        row = row.child(badge(t, z, format!("×{}", c.members)));
    }
    // An awake one's memory, else its state's word (a task says its word on its second line).
    let said = r.filter(|r| r.kind != WordKind::Task);
    if let Some((text, color)) = super::state::word(t, said) {
        row = row.child(
            div()
                .ml_auto()
                .flex_none()
                .text_size(z.p(12.))
                .text_color(color)
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
        .line_height(z.p(20.))
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
            .text_size(z.p(11.5));
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
                .text_size(z.p(11.5))
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
                .text_size(z.p(11.5))
                .text_color(t.dim)
                .child(SharedString::from(format!("+{}", c.more))),
        );
    }
    if c.fold {
        let f = m.folds.get(&c.name);
        let (glyph, color) = f.map_or(("✓", t.good), |f| (f.glyph, t.tone(f.tone)));
        row = row.child(
            div()
                .ml_auto()
                .flex_none()
                .text_size(z.p(11.5))
                .font_weight(w(700.))
                .text_color(color)
                .child(glyph),
        );
    }
    row
}

fn node_card(t: &Tokens, z: Z, m: &MapView, c: &Card, at: (f32, f32), mark: Mark) -> AnyElement {
    let r = m.rows.get(&c.name);
    let task = c.kind == Kind::Task;
    let mut f = frame(t, z, c, at, if task { 8. } else { 11. })
        .gap(z.p(7.))
        .px(z.p(13.))
        .py(z.p(11.));
    let st = super::state::frame(t, r, task);
    f = f.border_color(st.border).bg(st.fill);
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
        .child(top_row(t, z, c, r))
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
    f = f.border_color(st.border).bg(st.fill);
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
            .text_size(z.p(12.))
            .line_height(z.p(20.))
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
                .font_weight(w(600.))
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
        .child(top_row(t, z, c, r))
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
