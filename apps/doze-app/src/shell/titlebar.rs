//! The client-drawn title bar: Home and a tab per stack (its dot, its name, × on a stopped one), the bell, help and
//! ⌘K, with room for the traffic lights.

use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use super::{Root, Screen, Tab};
use crate::actions::{OpenPalette, ShowHelp};
use crate::kit::{chip_of, s};
use crate::theme::Tokens;
use crate::widgets::{dot, icon, kbd_plain, w};

fn tab(
    t: &Tokens,
    on: bool,
    id: impl Into<gpui::ElementId>,
    body: Vec<AnyElement>,
) -> gpui::Stateful<gpui::Div> {
    let base = div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(31.))
        .px(px(14.))
        .mb(px(-1.))
        .max_w(px(180.))
        .rounded_t(px(8.))
        .border_1()
        .border_b_0()
        .text_size(px(12.5))
        .whitespace_nowrap()
        .cursor_pointer()
        .children(body);
    if on {
        base.bg(t.bg)
            .border_color(t.line)
            .text_color(t.bright)
            .font_weight(w(600.))
    } else {
        base.border_color(gpui::transparent_black())
            .text_color(t.dim)
            .hover(|d| d.text_color(t.text))
    }
}

fn square(t: &Tokens, id: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .mb(px(6.))
        .flex_none()
        .w(px(32.))
        .h(px(30.))
        .rounded(px(7.))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|s| s.bg(t.sel))
}

/// The title bar.
pub(crate) fn titlebar(t: &Tokens, root: &Root, cx: &mut Context<Root>) -> impl IntoElement {
    let bell = square(t, "bell")
        .relative()
        .child(icon("icons/bell.svg", 16., t.dim));
    let help = square(t, "help")
        .text_size(px(13.))
        .font_weight(w(600.))
        .text_color(t.dim)
        .child("?")
        .on_click(cx.listener(|this, _, window, cx| this.on_help(&ShowHelp, window, cx)));
    div()
        .flex()
        .flex_none()
        .items_end()
        .gap(px(10.))
        .h(px(42.))
        .pl(px(86.))
        .pr(px(14.))
        .bg(t.side)
        .border_b_1()
        .border_color(t.line)
        .child(tabs(t, root, cx))
        .child(bell)
        .child(help)
        .child(search(t, cx))
}

/// The ⌘K button.
fn search(t: &Tokens, cx: &mut Context<Root>) -> impl IntoElement {
    div()
        .id("search")
        .mb(px(6.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(30.))
        .px(px(12.))
        .rounded(px(7.))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .text_size(px(12.))
        .font_weight(w(500.))
        .text_color(t.text)
        .cursor_pointer()
        .hover(|s| s.bg(t.sel))
        .on_click(cx.listener(|this, _, window, cx| this.on_palette(&OpenPalette, window, cx)))
        .child(kbd_plain(&chip_of("Search or do…"), 11., t.dim, 500.))
        .child("Search or do")
}

/// Home and each stack's tab.
fn tabs(t: &Tokens, root: &Root, cx: &mut Context<Root>) -> impl IntoElement {
    let home_on = root.screen == Screen::Library;
    let home = tab(
        t,
        home_on,
        "tab-home",
        vec![
            icon(
                "icons/home.svg",
                15.,
                if home_on { t.bright } else { t.dim },
            )
            .into_any_element(),
            div().child("Home").into_any_element(),
        ],
    )
    .on_click(cx.listener(|this, _, window, cx| this.go(Screen::Library, window, cx)));
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_end()
        .gap(px(2.))
        .child(home)
        .children(
            root.tabs
                .iter()
                .enumerate()
                .map(|(i, tb)| stack_tab(t, root, i, tb, cx)),
        )
}

fn stack_tab(t: &Tokens, root: &Root, i: usize, tb: &Tab, cx: &mut Context<Root>) -> AnyElement {
    let on = root.key() == Some(&tb.key);
    let tone = if tb.trouble {
        t.bad
    } else if tb.running {
        t.good
    } else {
        t.dim
    };
    let mut body = vec![
        dot(tone).into_any_element(),
        div()
            .overflow_hidden()
            .text_ellipsis()
            .child(s(&tb.name))
            .into_any_element(),
    ];
    if on || !tb.running {
        let k = tb.key.clone();
        body.push(
            div()
                .id(("tab-close", i))
                .size(px(16.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .justify_center()
                .text_color(t.dim)
                .hover(|d| d.bg(t.line).text_color(t.bright))
                .child(icon("icons/close.svg", 10., t.dim))
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.close_tab(&k, window, cx);
                }))
                .into_any_element(),
        );
    }
    let key = tb.key.clone();
    tab(t, on, ("tab", i), body)
        .on_click(
            cx.listener(move |this, _, window, cx| this.go(Screen::Stack(key.clone()), window, cx)),
        )
        .into_any_element()
}
