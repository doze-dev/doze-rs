//! The pages' shared parts, each from a rule in shell.css (named in its doc line): buttons, pills, the density
//! picker, the keys bar, section headings. Plain functions returning elements, as `widgets` are.

use gpui::{AnyElement, ClickEvent, Hsla, IntoElement, SharedString, Window, div, prelude::*, px};

use crate::actions;
use crate::theme::{Density, Tokens, mix};
use crate::widgets::{MONO_FAMILY, icon, kbd_chip, kbd_plain, tracked, w};

/// A click handler.
pub(crate) type Click = Box<dyn Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static>;

/// How a `.btn` is dressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Btn {
    /// `.btn`: bordered panel.
    Plain,
    /// `.btn.pri`: the accent.
    Primary,
    /// `.btn.quiet`: no frame, dim.
    Quiet,
}

/// The chip text of the first key of the row labelled `label`, or empty.
pub(crate) fn chip_of(label: &str) -> String {
    actions::all()
        .find(|s| s.label == label)
        .and_then(actions::chip)
        .unwrap_or_default()
}

/// `.btn`: 30 high, 12 across, 12/500, a key after the words in mono.
pub(crate) fn btn(
    t: &Tokens,
    id: impl Into<gpui::ElementId>,
    label: &str,
    key: &str,
    kind: Btn,
    click: Option<Click>,
) -> gpui::Stateful<gpui::Div> {
    let (bg, border, fg, key_c) = match kind {
        Btn::Plain => (t.panel, t.line2, t.text, t.dim),
        Btn::Primary => (t.accent, t.accent, t.ink, crate::theme::alpha(t.ink, 0.7)),
        Btn::Quiet => (
            gpui::transparent_black(),
            gpui::transparent_black(),
            t.dim,
            t.dim,
        ),
    };
    let hover_fg = if kind == Btn::Primary {
        t.ink
    } else {
        t.bright
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
        .text_size(px(12.))
        .font_weight(w(if kind == Btn::Primary { 600. } else { 500. }))
        .text_color(fg)
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(move |s| s.text_color(hover_fg))
        .child(SharedString::from(label.to_owned()));
    if !key.is_empty() {
        b = b.child(kbd_plain(key, 11., key_c, 500.));
    }
    if let Some(f) = click {
        b = b.on_click(f);
    }
    b
}

/// `.pill`: 22 high, rounded, panel, a dot when what it names is up.
pub(crate) fn pill(t: &Tokens, text: &str, up: bool, link: bool) -> gpui::Div {
    let mut p = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(22.))
        .px(px(9.))
        .rounded(px(11.))
        .bg(t.panel)
        .border_1()
        .border_color(t.line)
        .text_size(px(12.))
        .whitespace_nowrap()
        .overflow_hidden()
        .text_color(if up { t.text } else { t.dim });
    if up {
        p = p.child(crate::widgets::dot(t.good));
    }
    let words = div()
        .overflow_hidden()
        .text_ellipsis()
        .child(SharedString::from(text.to_owned()));
    p.child(if link {
        words.text_color(t.cool).underline()
    } else {
        words
    })
}

/// `.need`: the red pill, `●` and the words.
pub(crate) fn need(t: &Tokens, text: &str) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(5.))
        .h(px(22.))
        .px(px(8.))
        .rounded(px(11.))
        .bg(mix(t.bad, t.panel, 16.))
        .border_1()
        .border_color(mix(t.bad, t.panel, 40.))
        .text_color(t.bad)
        .text_size(px(12.))
        .font_weight(w(600.))
        .whitespace_nowrap()
        .overflow_hidden()
        .child("●")
        .child(
            div()
                .overflow_hidden()
                .text_ellipsis()
                .child(SharedString::from(text.to_owned())),
        )
}

/// The density picker: three bars as far apart as the rows would be (density.tsx), the chosen one lit.
pub(crate) fn density(
    t: &Tokens,
    on: Density,
    pick: impl Fn(Density) -> Click,
) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .rounded(px(7.))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .overflow_hidden()
        .children(Density::ALL.iter().enumerate().map(|(i, d)| {
            let path = match d {
                Density::Compact => "icons/lines-3.svg",
                Density::Cosy => "icons/lines-4.5.svg",
                Density::Roomy => "icons/lines-6.svg",
            };
            let lit = *d == on;
            div()
                .id(("density", i))
                .h(px(28.))
                .w(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .when(lit, |c| c.bg(t.sel))
                .when(i < 2, |c| c.border_r_1().border_color(t.line))
                .child(icon(path, 16., if lit { t.bright } else { t.dim }))
                .on_click(pick(*d))
        }))
}

/// `.keys`: the bar at the foot of a page, each chip from the action table, and a note on the right.
pub(crate) fn keys_bar(t: &Tokens, keys: &[(String, &str)], note: &str) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(16.))
        .h(px(34.))
        .px(px(28.))
        .border_t_1()
        .border_color(t.line)
        .text_size(px(12.))
        .text_color(t.dim)
        .whitespace_nowrap()
        .overflow_hidden()
        .children(keys.iter().filter(|(k, _)| !k.is_empty()).map(|(k, what)| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(5.))
                .child(kbd_chip(t, k, 11., 600., 6.))
                .child(SharedString::from((*what).to_owned()))
        }))
        .child(div().flex_1())
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .child(SharedString::from(note.to_owned())),
        )
}

/// An uppercase heading with tracking (`.gl`, `.qh`, `.insp .sec h3`).
pub(crate) fn heading(text: &str, color: Hsla, weight: f32, tracking: f32) -> impl IntoElement {
    tracked(text, 11., weight, color, tracking)
}

/// Text in the mono face.
pub(crate) fn mono(text: impl Into<SharedString>, size: f32, color: Hsla) -> gpui::Div {
    div()
        .font_family(MONO_FAMILY)
        .text_size(px(size))
        .text_color(color)
        .child(text.into())
}

/// The tone's colour (`.ok`, `.warn`, `.bad`, `.cool`, `.dim`, `.acc`; empty is the text's).
pub(crate) fn tone(t: &Tokens, tone: &str) -> Hsla {
    match tone {
        "ok" | "good" => t.good,
        "warn" => t.warn,
        "bad" => t.bad,
        "cool" => t.cool,
        "dim" => t.dim,
        "acc" | "accent" => t.accent,
        "bright" => t.bright,
        _ => t.text,
    }
}

/// `SharedString` from a `&str`.
pub(crate) fn s(text: &str) -> SharedString {
    SharedString::from(text.to_owned())
}

#[allow(dead_code)] // WHY: the any-element form the pages' children take
pub(crate) fn any(e: impl IntoElement) -> AnyElement {
    e.into_any_element()
}

/// A state word in its tone and weight, then the rest (` · why · what it said`), as one run of text, so it is cut with
/// an ellipsis where the room ends, as `text-overflow` cut the Go page's line.
pub(crate) fn word_line(word: &str, tone: Hsla, rest: &[&str]) -> gpui::StyledText {
    let mut text = word.to_owned();
    for r in rest.iter().filter(|r| !r.is_empty()) {
        text.push_str(" · ");
        text.push_str(r);
    }
    let style = gpui::HighlightStyle {
        color: Some(tone),
        font_weight: Some(w(600.)),
        ..Default::default()
    };
    gpui::StyledText::new(text).with_highlights([(0..word.len(), style)])
}
