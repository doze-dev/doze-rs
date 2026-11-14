//! "From your code": the module's connect line in the language you are in, with a copy button on the code.

use doze_desk::page::snippets::snippets as langs;
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use super::InstanceView;
use super::ui::{card, code, heading, text};
use crate::theme::{Tokens, ty};

pub(crate) fn snippets(
    t: &Tokens,
    v: &InstanceView,
    cx: &mut Context<InstanceView>,
) -> Option<AnyElement> {
    let all = langs(&v.page);
    if all.is_empty() {
        return None;
    }
    let at = v.lang.min(all.len() - 1);
    let (lang, body) = all.get(at)?.clone();
    let tabs = lang_tabs(t, &all, at, cx);
    let copy_btn = copy_button(t, v.is_copied("snippet"), (lang, &body), cx);
    let lines = snippet_lines(t, &body);
    Some(
        card(t)
            .gap(px(10.))
            .child(heading(t, "From your code", Some("· the same URL, in the language you are in")))
            .child(tabs)
            .child(code(t).child(lines).child(copy_btn))
            .child(div().text_size(px(ty::SM)).text_color(t.dim).child(
                "Inside the stack there is nothing to configure: the name resolves and the variable is there. The first connection wakes it if it is asleep.",
            ))
            .into_any_element(),
    )
}

/// The language tabs, the one at `at` underlined.
fn lang_tabs(
    t: &Tokens,
    all: &[(&'static str, String)],
    at: usize,
    cx: &mut Context<InstanceView>,
) -> gpui::Div {
    div()
        .flex()
        .gap(px(2.))
        .border_b_1()
        .border_color(t.line)
        .children(all.iter().enumerate().map(|(i, (name, _))| {
            let on = i == at;
            div()
                .id(("lang", i))
                .h(px(30.))
                .px(px(12.))
                .flex()
                .items_center()
                .mb(px(-1.))
                .border_b_2()
                .border_color(if on {
                    t.accent
                } else {
                    gpui::transparent_black()
                })
                .text_size(px(ty::SM))
                .font_weight(ty::MEDIUM)
                .text_color(if on { t.bright } else { t.dim })
                .cursor_pointer()
                .hover(|s| s.text_color(t.text))
                .on_click(cx.listener(move |v, _, _, cx| {
                    v.lang = i;
                    cx.notify();
                }))
                .child(*name)
        }))
}

/// The Copy button over the code.
fn copy_button(
    t: &Tokens,
    copied: bool,
    (lang, body): (&str, &str),
    cx: &mut Context<InstanceView>,
) -> gpui::Stateful<gpui::Div> {
    let what = doze_desk::copied::snippet(lang);
    div()
        .id("snippet-copy")
        .absolute()
        .top(px(8.))
        .right(px(8.))
        .h(px(24.))
        .px(px(9.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .text_size(px(ty::XS))
        .font_weight(ty::MEDIUM)
        .text_color(t.accent)
        .cursor_pointer()
        .child(if copied { "Copied" } else { "Copy" })
        .on_click(cx.listener({
            let body = body.to_owned();
            move |v, _, _, cx| v.copy("snippet", &body, &what, cx)
        }))
}

/// The snippet's lines, comments dimmed.
fn snippet_lines(t: &Tokens, body: &str) -> impl IntoElement {
    let lines = body.lines().map(|l| {
        let dim = l.trim_start().starts_with("//") || l.trim_start().starts_with('#');
        div()
            .min_h(px(19.))
            .text_color(if dim { t.dim } else { t.text })
            .child(text(l))
    });
    div()
        .id("snippet-lines")
        .flex()
        .flex_col()
        .pr(px(64.))
        .children(lines)
}
