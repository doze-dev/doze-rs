//! The words on the drawing: the captions over its parts (a focus view's column heads among them), and between far and
//! close, each card's name.

use doze_map::CardId;
use gpui::{AnyElement, Hsla, IntoElement, SharedString, Styled, div, prelude::*, px};

use super::MapView;
use super::cards::Z;
use crate::theme::{Tokens, alpha, mix};
use crate::widgets::{tracked, w};

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
