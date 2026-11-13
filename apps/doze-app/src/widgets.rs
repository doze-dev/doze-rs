//! The small parts every page draws, each from a rule in shell.css (named in
//! its doc line). Plain functions returning elements: no state, no hidden magic.

use gpui::{
    AnyElement, FontWeight, Hsla, IntoElement, SharedString, Styled, div, prelude::*, px, svg,
};

use crate::theme::Tokens;

pub(crate) const MONO_FAMILY: &str = "JetBrains Mono";
pub(crate) const UI_FAMILY: &str = "Inter";

pub(crate) fn w(n: f32) -> FontWeight {
    FontWeight(n)
}

/// An SVG icon (assets.rs) in a colour, `size` square.
pub(crate) fn icon(path: &'static str, size: f32, color: Hsla) -> impl IntoElement {
    svg()
        .path(path)
        .size(px(size))
        .flex_none()
        .text_color(color)
}

/// Uppercase small headings carry `letter-spacing`; GPUI has no tracking, so each
/// letter is its own box with the gap after it.
pub(crate) fn tracked(
    text: &str,
    size: f32,
    weight: f32,
    color: Hsla,
    tracking: f32,
) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .text_size(px(size))
        .font_weight(w(weight))
        .text_color(color)
        .children(
            text.to_uppercase()
                .chars()
                .map(|ch| {
                    div()
                        .mr(px((tracking - 0.5).max(0.)))
                        .child(SharedString::from(ch.to_string()))
                })
                .collect::<Vec<_>>(),
        )
}

/// `.dot` (8px disc) and `.ring` (8px, 2px cool border).
pub(crate) fn dot(color: Hsla) -> impl IntoElement {
    div().size(px(8.)).flex_none().rounded_full().bg(color)
}

/// `.sw`: 34x18, or `.sw.big` 44x24. The knob sits 2px in from the padding box.
pub(crate) fn switch(t: &Tokens, on: bool, big: bool, disabled: bool) -> impl IntoElement {
    let (width, height, knob, travel) = if big {
        (44., 24., 18., 23.)
    } else {
        (34., 18., 12., 18.)
    };
    let (bg, border, knob_bg) = if on {
        (t.accent, t.accent, t.ink)
    } else {
        (t.track, t.dim, t.dim)
    };
    let left = if on { travel } else { 2. };
    let top = if big { 2.5 } else { 2. };
    let sw = div()
        .relative()
        .flex_none()
        .w(px(width))
        .h(px(height))
        .rounded(px(height / 2.))
        .border_1()
        .border_color(border)
        .bg(bg)
        .child(
            div()
                .absolute()
                .top(px(top))
                .left(px(left))
                .size(px(knob))
                .rounded_full()
                .bg(knob_bg),
        );
    if disabled { sw.opacity(0.4) } else { sw }
}

/// `kbd` in a button or the keys bar: JetBrains Mono, `color`, no chrome.
pub(crate) fn kbd_plain(text: &str, size: f32, color: Hsla, weight: f32) -> impl IntoElement {
    div()
        .font_family(MONO_FAMILY)
        .text_size(px(size))
        .font_weight(w(weight))
        .text_color(color)
        .child(SharedString::from(text.to_string()))
}

/// `.keys kbd` / `.pal .foot kbd`: a chip on the selection colour.
pub(crate) fn kbd_chip(
    t: &Tokens,
    text: &str,
    size: f32,
    weight: f32,
    pad_x: f32,
) -> impl IntoElement {
    div()
        .flex_none()
        .px(px(pad_x))
        .py(px(1.))
        .rounded(px(4.))
        .bg(t.sel)
        .font_family(MONO_FAMILY)
        .text_size(px(size))
        .font_weight(w(weight))
        .text_color(t.bright)
        .child(SharedString::from(text.to_string()))
}

/// `.tag`: the NEW pill, 9px/700, .08em tracking.
pub(crate) fn tag(t: &Tokens, text: &str) -> impl IntoElement {
    div()
        .flex_none()
        .px(px(4.))
        .h(px(14.))
        .flex()
        .items_center()
        .rounded(px(3.))
        .bg(t.accent)
        .child(tracked(text, 9., 700., t.ink, 0.72))
}

/// What a segmented-control cell does when clicked.
pub(crate) type OnClick = crate::kit::Click;

/// A bordered, rounded segmented control; `items` are (child, is_on, click).
pub(crate) fn seg(
    t: &Tokens,
    id: &'static str,
    pad: f32,
    items: Vec<(AnyElement, bool, Option<OnClick>)>,
) -> impl IntoElement {
    let n = items.len();
    div()
        .flex()
        .flex_none()
        .rounded(px(7.))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .overflow_hidden()
        .children(
            items
                .into_iter()
                .enumerate()
                .map(|(i, (child, on, click))| {
                    let cell = div()
                        .id((id, i))
                        .h(px(28.))
                        .px(px(pad))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(12.))
                        .font_weight(w(500.))
                        .child(child);
                    let cell = if on {
                        cell.bg(t.sel).text_color(t.bright)
                    } else {
                        cell.text_color(t.dim)
                    };
                    let cell = if i + 1 < n {
                        cell.border_r_1().border_color(t.line)
                    } else {
                        cell
                    };
                    match click {
                        Some(f) => cell.on_click(f),
                        None => cell,
                    }
                }),
        )
}
