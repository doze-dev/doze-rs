//! Settings' list at the left (Go `.sets .side`): General, Machine (how many parts are to do), About (the version);
//! doze's version at the foot.

use doze_desk::settings::machine::{side_word, to_do};
use gpui::{Context, IntoElement, div, prelude::*, px};

use super::{Section, SettingsView};
use crate::instance::ui::text;
use crate::theme::Tokens;
use crate::widgets::{tracked, w};

pub(super) fn side(
    t: &Tokens,
    v: &SettingsView,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let machine = if v.parts.is_empty() {
        "checking…".to_owned()
    } else {
        side_word(&v.parts)
    };
    let warn = to_do(&v.parts) > 0;
    let on = |s: Section| v.section == s;
    div()
        .w(px(224.))
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(2.))
        .px(px(8.))
        .py(px(10.))
        .bg(t.side)
        .border_r_1()
        .border_color(t.line)
        .child(
            div()
                .mx(px(8.))
                .mt(px(6.))
                .mb(px(8.))
                .child(tracked("SETTINGS", 11., 600., t.dim, 0.88)),
        )
        .child(item(
            t,
            on(Section::General),
            (Section::General, String::new(), false),
            cx,
        ))
        .child(item(
            t,
            on(Section::Machine),
            (Section::Machine, machine, warn),
            cx,
        ))
        .child(item(
            t,
            on(Section::About),
            (Section::About, crate::VERSION.to_owned(), false),
            cx,
        ))
        .child(div().flex_1())
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .p(px(8.))
                .text_size(px(12.))
                .text_color(t.dim)
                .child(text(&format!("doze {}", crate::VERSION))),
        )
}

/// One part in the list: its name, and its small word at the right.
fn item(
    t: &Tokens,
    on: bool,
    (s, small, warn): (Section, String, bool),
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let small_color = if warn {
        t.warn
    } else if on {
        t.text
    } else {
        t.dim
    };
    div()
        .id(s.title())
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .h(px(30.))
        .px(px(8.))
        .rounded(px(7.))
        .whitespace_nowrap()
        .cursor_pointer()
        .text_color(if on { t.bright } else { t.text })
        .when(on, |d| d.bg(t.sel))
        .hover(|d| d.bg(t.sel))
        .on_click(cx.listener(move |v: &mut SettingsView, _, _, cx| v.show(s, false, cx)))
        .child(
            div()
                .flex_1()
                .when(on, |d| d.font_weight(w(600.)))
                .child(s.title()),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(11.5))
                .text_color(small_color)
                .child(text(&small)),
        )
}
