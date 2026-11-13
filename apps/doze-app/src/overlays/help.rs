//! The help sheet (`?`): every key by where it works, generated from the action table (so it lists exactly what is
//! bound, Go's Help board listed `o` and `⏎`, which nothing bound), and the words the app uses.

use gpui::{AnyElement, IntoElement, SharedString, div, prelude::*, px};

use crate::actions::{self, Spec};
use crate::theme::Tokens;
use crate::widgets::{UI_FAMILY, kbd_chip, tracked, w};

/// The words a person meets on the Map, and what each means (Go's help.tsx glossary, the Map's part).
const WORDS: [(&str, &str); 6] = [
    (
        "awake",
        "running, and something holds it: a caller, a connection, a pin",
    ),
    (
        "dozing",
        "running with nobody holding it: it sleeps when its idle time is up",
    ),
    ("asleep", "not running; the first call wakes it"),
    ("waking", "starting, because something called it"),
    ("calls", "a reference in doze.hcl from one app to another"),
    ("shared", "a module or a service the stack's apps use"),
];

fn row(t: &Tokens, s: &Spec) -> AnyElement {
    let keys = s.keys.iter().map(|k| crate::actions::key_words(k));
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(26.))
        .text_size(px(12.5))
        .child(
            div()
                .flex()
                .gap(px(4.))
                .w(px(76.))
                .flex_none()
                .children(keys.map(|k| kbd_chip(t, &k, 10.5, 600., 5.).into_any_element())),
        )
        .child(
            div()
                .flex_none()
                .text_color(if s.built { t.bright } else { t.dim })
                .font_weight(w(500.))
                .child(s.label),
        )
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(t.dim)
                .child(if s.built { s.what } else { "soon" }),
        )
        .into_any_element()
}

/// The words a person meets, and what each means.
fn words(t: &Tokens) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .pt(px(10.))
        .child(
            div()
                .pb(px(4.))
                .child(tracked("The words", 11., 600., t.dim, 0.66)),
        )
        .children(WORDS.iter().map(|(word, means)| {
            div()
                .flex()
                .gap(px(10.))
                .text_size(px(12.5))
                .line_height(px(22.))
                .child(
                    div()
                        .w(px(76.))
                        .flex_none()
                        .font_weight(w(600.))
                        .text_color(t.bright)
                        .child(*word),
                )
                .child(div().flex_1().min_w_0().text_color(t.text).child(*means))
        }))
}

/// The sheet's title line.
fn title(t: &Tokens, version: &str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .child(
            div()
                .text_size(px(16.))
                .font_weight(w(650.))
                .text_color(t.bright)
                .child("Keys and words"),
        )
        .child(div().flex_1())
        .child(
            div()
                .text_size(px(12.))
                .text_color(t.dim)
                .child(SharedString::from(format!("doze {version} · esc to close"))),
        )
}

/// The sheet's body.
pub(crate) fn sheet(t: &Tokens, version: &str) -> impl IntoElement {
    let mut cols = div().flex().gap(px(28.));
    use actions::Scope;
    let groups = actions::help();
    let (left, rest): (Vec<_>, Vec<_>) =
        groups.into_iter().partition(|(sc, _)| *sc == Scope::Global);
    let (inside, right): (Vec<_>, Vec<_>) = rest
        .into_iter()
        .partition(|(sc, _)| matches!(sc, Scope::Instance | Scope::Logs | Scope::Console));
    let column = |side: Vec<(actions::Scope, Vec<&'static Spec>)>| {
        let mut col = div().flex().flex_col().flex_1().min_w_0().gap(px(2.));
        for (scope, rows) in side {
            col = col.child(div().pt(px(6.)).pb(px(4.)).child(tracked(
                scope.title(),
                11.,
                600.,
                t.dim,
                0.66,
            )));
            col = col.children(rows.into_iter().map(|s| row(t, s)));
        }
        col
    };
    // Everywhere on the left; the Map, the search box and the words in the middle; inside a stack on the right. The
    // columns scroll when the window is too short for them.
    cols = cols
        .child(column(left))
        .child(column(right).child(words(t)))
        .child(column(inside));
    let cols = div()
        .id("help-keys")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(cols);
    div()
        .w(px(1220.))
        .max_w(gpui::relative(0.96))
        .max_h(gpui::relative(0.86))
        .flex()
        .flex_col()
        .gap(px(10.))
        .p(px(20.))
        .font_family(UI_FAMILY)
        .line_height(px(20.))
        .overflow_hidden()
        .rounded(px(14.))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .shadow(vec![super::shadow(t)])
        .child(title(t, version))
        .child(cols)
}
