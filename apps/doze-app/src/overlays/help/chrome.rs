//! The help sheet's frame around the keys: the head with its filter, the rail of contexts and the words, the foot.

use gpui::{AnyElement, Context, Entity, IntoElement, SharedString, Window, div, prelude::*, px};
use gpui_component::input::{Input, InputState};

use super::HelpView;
use super::model::{self, RAIL, Section};
use crate::theme::{Tokens, ty};
use crate::widgets::{Mono as _, framed_input, has_keys, kbd_chip};

/// The head: the title, and the filter at the right.
pub(super) fn head(
    t: &Tokens,
    input: &Entity<InputState>,
    window: &Window,
    cx: &Context<HelpView>,
) -> impl IntoElement {
    let focused = has_keys(input, window, cx);
    div()
        .flex()
        .items_center()
        .gap(px(16.))
        .px(px(24.))
        .pt(px(16.))
        .pb(px(14.))
        .border_b_1()
        .border_color(t.line)
        .child(
            div()
                .flex_none()
                .text_size(px(ty::LG))
                .font_weight(ty::HEADING)
                .text_color(t.bright)
                .child("Keys and words"),
        )
        .child(div().flex_1())
        .child(div().w(px(300.)).min_w(px(160.)).child(framed_input(
            t,
            "help/filter",
            focused,
            Input::new(input),
        )))
}

/// One rail entry: its name, and at the right "here", or while filtering how many matches it has.
fn rail_item(
    t: &Tokens,
    (s, on, here, count): (Section, bool, bool, Option<usize>),
    cx: &mut Context<HelpView>,
) -> AnyElement {
    let none = count == Some(0);
    let small = match count {
        Some(0) => String::new(),
        Some(n) => n.to_string(),
        None if here => "here".to_owned(),
        None => String::new(),
    };
    div()
        .id(SharedString::from(format!("help-rail-{}", s.short())))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .min_h(px(30.))
        .px(px(10.))
        .rounded(px(7.))
        .relative()
        .cursor_pointer()
        .text_color(if on {
            t.bright
        } else if none {
            t.dim
        } else {
            t.text
        })
        .when(on, |d| d.bg(t.sel).child(crate::widgets::sel_bar(t, 7.)))
        .hover(|d| d.bg(t.sel))
        .on_click(cx.listener(move |v: &mut HelpView, _, window, cx| v.show(s, window, cx)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .when(on, |d| d.font_weight(ty::SEMIBOLD))
                .child(s.short()),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(ty::XS_UP))
                .text_color(if count.is_some() { t.text } else { t.dim })
                .child(SharedString::from(small)),
        )
        .into_any_element()
}

/// The rail: every context, then the words; the one shown marked, and the page's own says "here".
pub(super) fn rail(
    t: &Tokens,
    v: &HelpView,
    filtering: bool,
    cx: &mut Context<HelpView>,
) -> impl IntoElement {
    let here = model::opening(v.here);
    let mut d = div()
        .id("help-rail")
        .w(px(super::RAIL_W))
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(2.))
        .px(px(10.))
        .py(px(12.))
        .overflow_y_scroll()
        .border_r_1()
        .border_color(t.line);
    for s in RAIL {
        if s == Section::Words {
            d = d.child(div().h(px(1.)).mx(px(6.)).my(px(8.)).bg(t.line));
        }
        let count = filtering.then(|| v.found.count(s));
        let on = !filtering && v.on == s;
        d = d.child(rail_item(t, (s, on, s == here, count), cx));
    }
    d
}

/// The foot: the keys are commands too, the version, esc (which clears the filter first).
pub(super) fn foot(t: &Tokens, version: &str, filtering: bool) -> impl IntoElement {
    let cmd = |s: &'static str| div().mono_face().text_color(t.text).child(s);
    div()
        .flex()
        .items_center()
        .gap(px(16.))
        .px(px(24.))
        .py(px(12.))
        .border_t_1()
        .border_color(t.line)
        .text_size(px(ty::SM))
        .text_color(t.dim)
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .flex_wrap()
                .items_center()
                .gap_x(px(6.))
                .child("Every key here is a doze command too:")
                .child(div().flex().child(cmd("doze sleep db")).child(","))
                .child(cmd("doze logs api"))
                .child("· scripts and CI use those.")
                .child("Letters are off while a console has focus."),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .child(SharedString::from(format!("doze {version}")))
                .child(div().w(px(4.)))
                .child(kbd_chip(t, "esc", ty::MICRO, ty::SEMIBOLD, 5.))
                .child(if filtering { "clear" } else { "close" }),
        )
}
