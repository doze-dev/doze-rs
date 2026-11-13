//! The client-drawn title bar: Home and the stack's tab, the bell, help and ⌘K, with room for the traffic lights.

use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use super::Root;
use super::chrome::{chip_of, text};
use crate::actions::{OpenPalette, ShowHelp};
use crate::model::Page;
use crate::theme::Tokens;
use crate::widgets::{dot, icon, kbd_plain, w};

fn tab(t: &Tokens, on: bool, body: Vec<AnyElement>) -> impl IntoElement {
    let base = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(31.))
        .px(px(14.))
        .mb(px(-1.))
        .rounded_t(px(8.))
        .border_1()
        .border_b_0()
        .text_size(px(12.5))
        .whitespace_nowrap()
        .children(body);
    if on {
        base.bg(t.bg).border_color(t.line).text_color(t.bright)
    } else {
        base.border_color(gpui::transparent_black())
            .text_color(t.dim)
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

/// The title bar: Home and the stack's tab; the bell, help and ⌘K. Leaves room for the traffic lights.
pub(crate) fn titlebar(t: &Tokens, page: &Page, cx: &mut Context<Root>) -> impl IntoElement {
    let tone = if page.vitals.trouble > 0 {
        t.bad
    } else if page.running {
        t.good
    } else {
        t.dim
    };
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
        .child(tabs(t, page, tone))
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

/// Home and the stack's tab.
fn tabs(t: &Tokens, page: &Page, tone: gpui::Hsla) -> impl IntoElement {
    div()
        .flex()
        .flex_1()
        .items_end()
        .gap(px(2.))
        .child(tab(
            t,
            false,
            vec![
                icon("icons/home.svg", 15., t.dim).into_any_element(),
                div().child("Home").into_any_element(),
            ],
        ))
        .child(tab(
            t,
            true,
            vec![
                dot(tone).into_any_element(),
                div()
                    .font_weight(w(600.))
                    .child(text(&page.name))
                    .into_any_element(),
            ],
        ))
}
