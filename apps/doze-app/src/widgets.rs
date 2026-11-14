//! The small parts every page draws, each from a rule in shell.css (named in
//! its doc line). Plain functions returning elements: no state, no hidden magic.

use gpui::{
    AnyElement, FontWeight, Hsla, IntoElement, SharedString, Styled, div, prelude::*, px, svg,
};

use crate::theme::{Tokens, ty};

pub(crate) const MONO_FAMILY: &str = "Geist Mono";
pub(crate) const UI_FAMILY: &str = "Geist";

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
    weight: FontWeight,
    color: Hsla,
    tracking: f32,
) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .text_size(px(size))
        .font_weight(weight)
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

/// A hover title (Go's `title=`): a tooltip with `words`, for `.tooltip(tip(...))` on an element with an id. GPUI has
/// no accessibility tree, so Go's `aria-label`s have no counterpart; the title is what a pointer still gets.
pub(crate) fn tip(
    words: impl Into<SharedString>,
) -> impl Fn(&mut gpui::Window, &mut gpui::App) -> gpui::AnyView + 'static {
    let words: SharedString = words.into();
    move |window, cx| gpui_component::tooltip::Tooltip::new(words.clone()).build(window, cx)
}

/// `.dot` (8px disc) and `.ring` (8px, 2px cool border).
pub(crate) fn dot(color: Hsla) -> impl IntoElement {
    div().size(px(8.)).flex_none().rounded_full().bg(color)
}

/// `.sw`: 34x18, or `.sw.big` 44x24. The knob sits 2px in from the padding box. Only the knob moves when it flips
/// (`left .12s ease-out`, remembered by `id`); the fill and the border change at once, as Go's do.
pub(crate) fn switch(
    t: &Tokens,
    id: &str,
    on: bool,
    big: bool,
    disabled: bool,
) -> impl IntoElement {
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
    let (left, moving) = crate::motion::slide(id, if on { travel } else { 2. });
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
        )
        .child(crate::motion::tick(moving));
    if disabled { sw.opacity(0.4) } else { sw }
}

/// The key symbols drawn a size up: at a chip's 10 to 12 px they are a letter's height and read as specks beside it.
const KEY_SYMBOLS: [char; 4] = ['↵', '⌫', '⇧', '⌘'];

/// A key's words as drawn, in runs: letters at `size`, the symbols at `size + KEY_GLYPH_UP`, centred on one line.
/// Return is Geist Mono's `↵`, a plain arrow at the letters' weight; its `⏎` is a hollow keycap arrow that blurs at
/// this size.
fn key_runs(text: &str, size: f32) -> Vec<AnyElement> {
    let text = text.replace('⏎', "↵");
    let mut runs: Vec<(bool, String)> = Vec::new();
    for ch in text.chars() {
        let sym = KEY_SYMBOLS.contains(&ch);
        match runs.last_mut() {
            Some((s, run)) if *s == sym => run.push(ch),
            _ => runs.push((sym, ch.to_string())),
        }
    }
    runs.into_iter()
        .map(|(sym, run)| {
            let glyph = if sym { size + ty::KEY_GLYPH_UP } else { size };
            div()
                .flex_none()
                .text_size(px(glyph))
                .child(SharedString::from(run))
                .into_any_element()
        })
        .collect()
}

/// `kbd` in a button or the keys bar: Geist Mono (it draws ↵ ⌫ ⇧ and the arrows itself), `color`, no chrome.
pub(crate) fn kbd_plain(
    text: &str,
    size: f32,
    color: Hsla,
    weight: FontWeight,
) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .font_family(MONO_FAMILY)
        .text_size(px(size))
        .font_weight(weight)
        .text_color(color)
        .children(key_runs(text, size))
}

/// `.keys kbd` / `.pal .foot kbd`: a chip on the selection colour.
pub(crate) fn kbd_chip(
    t: &Tokens,
    text: &str,
    size: f32,
    weight: FontWeight,
    pad_x: f32,
) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .px(px(pad_x))
        .py(px(1.))
        .rounded(px(4.))
        .bg(t.sel)
        .font_family(MONO_FAMILY)
        // The chip keeps its height: a larger symbol sits in the same line.
        .line_height(px(size + ty::CHIP_LEAD))
        .text_size(px(size))
        .font_weight(weight)
        .text_color(t.bright)
        .children(key_runs(text, size))
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
        .child(tracked(text, ty::TAG, ty::BOLD, t.ink, 0.72))
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
                        .text_size(px(ty::SM))
                        .font_weight(ty::MEDIUM)
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
