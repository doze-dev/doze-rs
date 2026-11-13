//! The Map's cards up close, as flow.tsx and `.map` in shell.css drew them, scaled by the zoom (CSS `zoom`: every
//! length together) and placed in the viewport; only the cards in view are built. The captions, and between far and
//! close, the cards' names.

use doze_display::{Kind as WordKind, Look};
use doze_map::{Card, CardId, Kind, Role};
use gpui::{
    AnyElement, BoxShadow, Hsla, IntoElement, SharedString, Styled, div, point, prelude::*, px,
};

use super::MapView;
use crate::model::Row;
use crate::theme::{Tokens, alpha, mix};
use crate::widgets::{MONO_FAMILY, tracked, w};

/// Lengths scaled by the zoom.
#[derive(Clone, Copy)]
struct Z(f32);

impl Z {
    fn p(self, v: f32) -> gpui::Pixels {
        px(v * self.0)
    }
}

/// The state dot: filled awake (good) or dozing (warn), a ring while waking, red in trouble, dim asleep.
pub(crate) fn state_dot(t: &Tokens, size: gpui::Pixels, r: Option<&Row>) -> AnyElement {
    let d = div().size(size).flex_none().rounded_full();
    let Some(r) = r else {
        return d.bg(t.dim).into_any_element();
    };
    if r.trouble {
        return d.bg(t.bad).into_any_element();
    }
    match r.look {
        Look::Awake => d.bg(t.good),
        Look::Dozing => d.bg(t.warn),
        Look::Waking => d.border(size / 4.).border_color(t.cool),
        _ => d.bg(t.dim),
    }
    .into_any_element()
}

fn task_glyph(t: &Tokens, z: Z, r: &Row) -> AnyElement {
    div()
        .w(z.p(8.))
        .text_size(z.p(12.))
        .text_color(t.tone(r.look.tone()))
        .child(r.look.glyph())
        .into_any_element()
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
    let dot = match r {
        Some(r) if r.kind == WordKind::Task => task_glyph(t, z, r),
        _ => state_dot(t, z.p(8.), r),
    };
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
    if let Some(mem) = r
        .map(|r| r.mem.clone())
        .filter(|m| !m.is_empty() && m != "0")
    {
        row = row.child(
            div()
                .ml_auto()
                .flex_none()
                .text_size(z.p(12.))
                .text_color(t.dim)
                .child(SharedString::from(mem)),
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
    // Opaque, blended toward the well: a translucent spread shadow tints the card it rings.
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
    if task {
        f = f.border_dashed();
    }
    if r.is_some_and(|r| r.trouble) {
        f = f.border_color(mix(t.bad, t.line, 55.));
    }
    if mark.hot > 0.0 {
        let was = if r.is_some_and(|r| r.trouble) {
            mix(t.bad, t.line, 55.)
        } else {
            t.line
        };
        f = f.border_color(mix(t.accent, was, mark.hot * 100.0));
        // The ring as Go draws a selection (`box-shadow`, not transitioned): there once the blend is, so a
        // translucent shadow never tints the card through.
        if mark.hot >= 1.0 || mark.sel {
            f = f.shadow(ring(t, z, mark.sel, 1.0));
        }
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
    if r.is_some_and(|r| r.trouble) {
        f = f.border_color(mix(t.bad, t.line, 55.));
    }
    if mark.hot > 0.0 {
        let was = if r.is_some_and(|r| r.trouble) {
            mix(t.bad, t.line, 55.)
        } else {
            t.line
        };
        f = f.border_color(mix(t.accent, was, mark.hot * 100.0));
        // The ring as Go draws a selection (`box-shadow`, not transitioned): there once the blend is, so a
        // translucent shadow never tints the card through.
        if mark.hot >= 1.0 || mark.sel {
            f = f.shadow(ring(t, z, false, 1.0));
        }
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

/// The captions.
pub(crate) fn captions(t: &Tokens, m: &MapView) -> Vec<AnyElement> {
    // A focus view's column heads read at any zoom; a small map's scale with it, as Go's did.
    let z = if m.layout.mode == doze_map::Mode::Focus {
        Z(m.cam.zoom.max(0.9))
    } else {
        Z(m.cam.zoom)
    };
    let band = m.layout.band;
    m.layout
        .captions
        .iter()
        .map(|c| {
            let in_band = band.is_some_and(|b| c.at.y > b);
            let (x, y) = m.cam.to_screen(c.at.x - 4.0, c.at.y);
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .flex()
                .items_center()
                .h(z.p(16.))
                .line_height(z.p(16.))
                .px(z.p(4.))
                .rounded(z.p(4.))
                .bg(if in_band {
                    mix(t.panel, t.well, 45.)
                } else {
                    t.well
                })
                .whitespace_nowrap()
                .child(tracked(&c.text, 11. * z.0, 700., t.dim, 1.1 * z.0))
                .child(
                    div()
                        .ml(z.p(6.))
                        .text_size(z.p(11.))
                        .text_color(alpha(t.dim, 0.85))
                        .child(SharedString::from(c.sub.clone())),
                )
                .into_any_element()
        })
        .collect()
}

/// Between blocks and full cards: each card in view named, one line, at a size still readable.
pub(crate) fn labels(
    t: &Tokens,
    m: &MapView,
    vis: &[CardId],
    on: Option<&[bool]>,
) -> Vec<AnyElement> {
    let z = m.cam.zoom;
    let size = (14.0 * z).max(9.0);
    vis.iter()
        .filter_map(|&id| {
            let c = m.layout.card(id)?;
            if (c.rect.h as f32 * z) < size + 4.0 {
                return None;
            }
            let faded = m
                .fades
                .opacity(on, id.ix(), m.fades.lit(std::time::Instant::now()))
                < 1.0;
            // In a region's frame its name is said once, on the frame: `checkout-cart` reads `cart`.
            let short = m
                .layout
                .region(id)
                .and_then(|r| c.name.strip_prefix(r.name.as_str()))
                .and_then(|s| s.strip_prefix(['-', '_', '.']))
                .filter(|s| !s.is_empty())
                .unwrap_or(&c.name);
            let name = if c.members > 1 {
                format!("{short} ×{}", c.members)
            } else {
                short.to_string()
            };
            let (x, y) = m.cam.to_screen(c.rect.x, c.rect.y);
            let color: Hsla = if faded { alpha(t.dim, 0.5) } else { t.bright };
            Some(
                div()
                    .absolute()
                    .left(px(x + (c.rect.w as f32 * z * 0.05).max(2.0) + 4.0))
                    .top(px(y + 3.0))
                    .w(px(c.rect.w as f32 * z * 0.9 - 6.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(size))
                    .line_height(px(size + 2.0))
                    .font_weight(w(600.))
                    .text_color(color)
                    .child(SharedString::from(name))
                    .into_any_element(),
            )
        })
        .collect()
}
