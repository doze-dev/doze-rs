//! The parts the pages inside a stack share, each from a rule in the Go app's shell.css (named in its doc line):
//! cards and their headings, key/value rows with a copy button, buttons with their key, pills, banners, the state
//! dot. Plain functions returning elements.

use doze_desk::page::Ink;
use doze_desk::page::parts::Kv;
use doze_display::Look;
use gpui::{AnyElement, Div, Hsla, IntoElement, SharedString, Stateful, div, prelude::*, px};

use crate::theme::{Tokens, mix, ty};
use crate::widgets::{MONO_FAMILY, dot, tracked};

/// A colour class as a colour.
pub(crate) fn ink(t: &Tokens, i: Ink) -> Hsla {
    match i {
        Ink::Good => t.good,
        Ink::Warn => t.warn,
        Ink::Cool => t.cool,
        Ink::Bad => t.bad,
        Ink::Dim => t.dim,
        Ink::Text => t.text,
        Ink::Bright => t.bright,
        Ink::Accent => t.accent,
    }
}

pub(crate) fn text(s: &str) -> SharedString {
    SharedString::from(s.to_owned())
}

/// `.card`: the panel, a line round it, 10px corners.
pub(crate) fn card(t: &Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .min_w_0()
        .px(px(16.))
        .py(px(14.))
        .rounded(px(10.))
        .border_1()
        .border_color(t.line)
        .bg(t.panel)
}

/// `.card h3`: the heading, tracked capitals, with words after it in plain case.
pub(crate) fn heading(t: &Tokens, title: &str, sub: Option<&str>) -> Div {
    let mut h = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .whitespace_nowrap()
        .child(tracked(title, ty::SM, ty::SEMIBOLD, t.dim, 0.72));
    if let Some(s) = sub.filter(|s| !s.is_empty()) {
        h = h.child(div().text_size(px(ty::SM)).text_color(t.dim).child(text(s)));
    }
    h
}

/// `.copy`: a quiet accent link.
pub(crate) fn copy_link(t: &Tokens, id: impl Into<gpui::ElementId>, label: &str) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .text_size(px(ty::XS))
        .font_weight(ty::MEDIUM)
        .text_color(t.accent)
        .cursor_pointer()
        .hover(|s| s.underline())
        .child(text(label))
}

/// `.kv`: key, value, and a copy link when there is something to copy.
pub(crate) fn kv_row(
    t: &Tokens,
    r: &Kv,
    copied: bool,
    on_copy: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> Div {
    let value = div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .text_color(if r.dim { t.dim } else { t.bright })
        .when(r.mono, |d| d.font_family(MONO_FAMILY).text_size(px(ty::SM)))
        .child(text(&r.v));
    let mut row = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .min_h(px(26.))
        .child(
            div()
                .w(px(96.))
                .flex_none()
                .text_color(t.dim)
                .child(text(&r.k)),
        )
        .child(value);
    if !r.copy.is_empty() {
        let label = match (copied, r.k.as_str()) {
            (true, _) => "Copied",
            (false, "Env var") => "Copy export",
            _ => "Copy",
        };
        row = row.child(
            copy_link(t, SharedString::from(format!("copy-{}", r.k)), label).on_click(on_copy),
        );
    }
    if r.note.is_empty() {
        return row;
    }
    div().flex().flex_col().child(row).child(
        div()
            .pl(px(108.))
            .mt(px(-4.))
            .mb(px(4.))
            .text_size(px(ty::XS))
            .line_height(px(ty::LH_15))
            .text_color(t.dim)
            .child(text(&r.note)),
    )
}

/// `.btn`, `.btn.pri`: a button with its key beside the label.
pub(crate) fn button(
    t: &Tokens,
    id: &'static str,
    label: &str,
    key: &str,
    primary: bool,
) -> Stateful<Div> {
    let (bg, border, fg, kfg) = if primary {
        (t.accent, t.accent, t.ink, mix(t.ink, t.accent, 70.))
    } else {
        (t.panel, t.line2, t.text, t.dim)
    };
    let mut b = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(30.))
        .px(px(12.))
        .rounded(px(7.))
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_color(fg)
        .text_size(px(ty::SM))
        .font_weight(if primary { ty::SEMIBOLD } else { ty::MEDIUM })
        .whitespace_nowrap()
        .cursor_pointer()
        .child(text(label));
    b = if primary {
        b.hover(|s| s.opacity(0.92))
    } else {
        b.hover(|s| s.border_color(t.dim).text_color(t.bright))
    };
    if key.is_empty() {
        return b;
    }
    b.child(
        div()
            .font_family(MONO_FAMILY)
            .text_size(px(ty::XS))
            .text_color(kfg)
            .child(text(key)),
    )
}

/// `.pill`: rounded, with a dot.
pub(crate) fn pill(t: &Tokens, color: Hsla, body: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(22.))
        .px(px(9.))
        .rounded(px(11.))
        .border_1()
        .border_color(t.line)
        .bg(t.panel)
        .text_size(px(ty::SM))
        .whitespace_nowrap()
        .child(dot(color))
        .child(body)
}

/// A banner's tone.
#[derive(Clone, Copy)]
pub(crate) enum Tone {
    Bad,
}

/// `.banner.bad`, `.banner.warn`: a tinted panel with a dot, a line in bold and a line under it.
pub(crate) fn banner(t: &Tokens, tone: Tone, title: &str, sub: &str) -> Div {
    let c = match tone {
        Tone::Bad => t.bad,
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .px(px(14.))
        .py(px(10.))
        .rounded(px(10.))
        .border_1()
        .border_color(mix(c, t.panel, 50.))
        .bg(mix(c, t.panel, 10.))
        .child(dot(c))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(1.))
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_color(t.bright)
                        .font_weight(ty::SEMIBOLD)
                        .child(text(title)),
                )
                .when(!sub.is_empty(), |d| {
                    d.child(
                        div()
                            .text_size(px(ty::SM))
                            .text_color(t.dim)
                            .child(text(sub)),
                    )
                }),
        )
}

/// An instance's state as the boards draw it: a filled dot, a ring while waking, a glyph for a task.
pub(crate) fn state_mark(t: &Tokens, look: Option<Look>, trouble: bool, size: f32) -> AnyElement {
    let disc = |c: Hsla| {
        div()
            .size(px(size))
            .flex_none()
            .rounded_full()
            .bg(c)
            .into_any_element()
    };
    if trouble {
        return disc(t.bad);
    }
    match look {
        Some(Look::Awake) => disc(t.good),
        Some(Look::Dozing) => disc(t.warn),
        Some(Look::Waking | Look::Running) => div()
            .size(px(size))
            .flex_none()
            .rounded_full()
            .border_2()
            .border_color(t.cool)
            .into_any_element(),
        Some(Look::Asleep | Look::Off) | None => disc(t.dim),
        Some(l) => div()
            .w(px(size + 2.))
            .flex_none()
            .text_size(px(ty::SM))
            .text_color(t.tone(l.tone()))
            .child(l.glyph())
            .into_any_element(),
    }
}

/// `.step-num`: a number in a ring.
pub(crate) fn step_num(t: &Tokens, n: usize) -> Div {
    div()
        .size(px(22.))
        .flex_none()
        .rounded_full()
        .border_1()
        .border_color(t.line2)
        .flex()
        .items_center()
        .justify_center()
        .font_family(MONO_FAMILY)
        .text_size(px(ty::XS))
        .font_weight(ty::SEMIBOLD)
        .text_color(t.dim)
        .child(text(&n.to_string()))
}

/// `.code`: monospaced, in the well.
pub(crate) fn code(t: &Tokens) -> Div {
    div()
        .relative()
        .flex()
        .flex_col()
        .px(px(14.))
        .py(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(t.line)
        .bg(t.well)
        .font_family(MONO_FAMILY)
        .text_size(px(ty::SM))
        .line_height(px(ty::LH_19))
        .text_color(t.text)
}
