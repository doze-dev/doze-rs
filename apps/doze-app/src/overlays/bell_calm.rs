//! Needs you when nothing does (the BellEmpty board): a ring with a tick, what it means, and what would show here.

use gpui::{AnyElement, IntoElement, SharedString, div, prelude::*, px};

use crate::theme::{Tokens, alpha, ty};

/// One line of the legend: a coloured word, and what it covers.
fn legend(t: &Tokens, red: bool, k: &str, v: &str) -> gpui::Div {
    let c = if red { t.bad } else { t.warn };
    let word = div()
        .w(px(96.))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .font_weight(ty::SEMIBOLD)
        .text_color(c)
        .child(div().size(px(7.)).rounded_full().bg(c))
        .child(SharedString::from(k.to_owned()));
    div().flex().gap(px(12.)).child(word).child(
        div()
            .flex_1()
            .min_w_0()
            .text_color(t.dim)
            .child(SharedString::from(v.to_owned())),
    )
}

/// The ring, the title and the line under it.
fn top(t: &Tokens, title: String, said: String) -> gpui::Div {
    let ring = div()
        .size(px(44.))
        .rounded_full()
        .border_2()
        .border_color(t.good)
        .bg(alpha(t.good, 0.1))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(ty::LG_UP))
        .text_color(t.good)
        .child("✓");
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(6.))
        .pt(px(26.))
        .pb(px(18.))
        .child(ring)
        .child(
            div()
                .mt(px(6.))
                .text_size(px(ty::MD_UP))
                .font_weight(ty::HEADING)
                .text_color(t.bright)
                .child(SharedString::from(title)),
        )
        .child(
            div()
                .text_size(px(ty::SM_UP))
                .text_color(t.dim)
                .child(SharedString::from(said)),
        )
}

/// The calm panel's body; `hidden` items were put aside with Not now.
pub(crate) fn calm(t: &Tokens, hidden: usize) -> AnyElement {
    let (title, said) = if hidden > 0 {
        (
            "Nothing new needs you.".to_owned(),
            format!("{hidden} put aside with Not now; each comes back when what it says changes."),
        )
    } else {
        (
            "Nothing needs you.".to_owned(),
            "Every stack is doing what you asked.".to_owned(),
        )
    };
    let what = div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .mx(px(16.))
        .mb(px(14.))
        .p(px(12.))
        .rounded(px(9.))
        .bg(t.well)
        .text_size(px(ty::SM))
        .child(legend(
            t,
            true,
            "Broken now",
            "a crash, a failed task, a service that did not wake in time, a stack that failed to start, a daemon not answering. These also light the tab, the bat and a notification.",
        ))
        .child(legend(
            t,
            false,
            "Worth doing",
            "doze.hcl changed or not loading, a domain or a machine part not set up, things left behind, a stack whose folder is gone. These wait here quietly.",
        ));
    div()
        .flex()
        .flex_col()
        .child(top(t, title, said))
        .child(what)
        .into_any_element()
}
