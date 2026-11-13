//! The sidebar inside a stack, as the inside boards draw it: ‹ Overview, then the stack's instances under their groups,
//! each with its state and a word or figure; on All logs, the Show list instead, each instance ticked or not, and
//! doze itself. Generic over the view that owns it, so the shell draws it beside any page.

use std::collections::BTreeSet;

use doze_desk::page::side::Side;
use doze_display::Group;
use gpui::{Context, Div, IntoElement, Window, div, prelude::*, px};

use super::ui::{state_mark, text};
use crate::theme::Tokens;
use crate::widgets::{tracked, w};

/// What the sidebar shows besides the instances.
pub(crate) enum Mode<'a> {
    /// An instance's page: this one is selected.
    Instance(&'a str),
    /// All logs: which instances are shown (`None`: all).
    Show(&'a Option<BTreeSet<String>>),
}

/// What a click in the sidebar means.
#[derive(Debug, Clone)]
pub(crate) enum Pick {
    Overview,
    Instance(String),
    Toggle(String),
}

fn frame(t: &Tokens) -> Div {
    div()
        .w(px(224.))
        .flex_none()
        .flex()
        .flex_col()
        .px(px(8.))
        .py(px(10.))
        .bg(t.side)
        .border_r_1()
        .border_color(t.line)
        .overflow_hidden()
}

fn item(t: &Tokens, on: bool) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .h(px(30.))
        .px(px(8.))
        .rounded(px(7.))
        .whitespace_nowrap()
        .text_color(if on { t.bright } else { t.text })
        .when(on, |d| d.bg(t.sel))
        .hover(|s| s.bg(t.sel))
        .cursor_pointer()
}

fn group_head(t: &Tokens, g: Group) -> Div {
    div()
        .mx(px(8.))
        .mt(px(10.))
        .mb(px(4.))
        .child(tracked(g.title(), 11., 600., t.dim, 0.88))
}

/// The sidebar; `pick` hears its clicks.
pub(crate) fn sidebar<T: 'static>(
    t: &Tokens,
    sides: &[Side],
    mode: Mode<'_>,
    foot: &str,
    cx: &mut Context<T>,
    pick: fn(&mut T, Pick, &mut Window, &mut Context<T>),
) -> impl IntoElement {
    let back = item(t, false)
        .id("side-overview")
        .on_click(cx.listener(move |this, _, w, cx| pick(this, Pick::Overview, w, cx)))
        .child(div().w(px(8.)).text_color(t.dim).child("‹"))
        .child(div().flex_1().child("Overview"));
    // The list scrolls on a long stack; Overview and the foot stay put.
    let list = div().flex().flex_col().min_h_0().flex_1();
    let list = match mode {
        Mode::Instance(current) => instances(list, t, sides, current, cx, pick),
        Mode::Show(show) => show_list(list, t, sides, show, cx, pick),
    };
    frame(t)
        .child(back)
        .child(
            div()
                .id("side-list")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(list),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .p(px(8.))
                .text_size(px(12.))
                .text_color(t.dim)
                .whitespace_nowrap()
                .child(gpui::img("brand/doze.svg").size(px(18.)).flex_none())
                .child(text(foot)),
        )
}

/// The stack's instances under their groups, `current` selected, each with its state and a word or figure.
fn instances<T: 'static>(
    mut s: Div,
    t: &Tokens,
    sides: &[Side],
    current: &str,
    cx: &mut Context<T>,
    pick: fn(&mut T, Pick, &mut Window, &mut Context<T>),
) -> Div {
    for g in Group::ALL {
        let rows: Vec<&Side> = sides.iter().filter(|x| x.group == g).collect();
        if rows.is_empty() {
            continue;
        }
        s = s.child(group_head(t, g));
        for (i, x) in rows.into_iter().enumerate() {
            let name = x.name.clone();
            s = s.child(
                item(t, x.name == current)
                    .id((g.title(), i))
                    .on_click(cx.listener(move |this, _, w, cx| {
                        pick(this, Pick::Instance(name.clone()), w, cx)
                    }))
                    .child(state_mark(t, Some(x.look), x.trouble, 8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .when(x.name == current, |d| d.font_weight(w(600.)))
                            .when(x.look == doze_display::Look::Off, |d| d.text_color(t.dim))
                            .child(text(&x.name)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.5))
                            .text_color(if x.trouble { t.bad } else { t.dim })
                            .child(text(if x.trouble { x.look.word() } else { &x.note })),
                    ),
            );
        }
    }
    s
}

/// All logs' Show list: each instance ticked or not, and doze itself.
fn show_list<T: 'static>(
    mut s: Div,
    t: &Tokens,
    sides: &[Side],
    show: &Option<BTreeSet<String>>,
    cx: &mut Context<T>,
    pick: fn(&mut T, Pick, &mut Window, &mut Context<T>),
) -> Div {
    s = s.child(item(t, true).child(div().font_weight(w(600.)).child("All logs")));
    s = s.child(
        div()
            .mx(px(8.))
            .mt(px(10.))
            .mb(px(4.))
            .child(tracked("Show", 11., 600., t.dim, 0.88)),
    );
    let on = |n: &str| show.as_ref().is_none_or(|s| s.contains(n));
    for (i, x) in sides.iter().enumerate() {
        let name = x.name.clone();
        s =
            s.child(
                item(t, false)
                    .id(("show", i))
                    .on_click(cx.listener(move |this, _, w, cx| {
                        pick(this, Pick::Toggle(name.clone()), w, cx)
                    }))
                    .child(tick(t, on(&x.name)))
                    .child(state_mark(t, Some(x.look), x.trouble, 8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(text(&x.name)),
                    ),
            );
    }
    s.child(
        item(t, false)
            .id("show-doze")
            .on_click(
                cx.listener(move |this, _, w, cx| pick(this, Pick::Toggle("doze".into()), w, cx)),
            )
            .child(tick(t, on("doze")))
            .child(
                div()
                    .w(px(8.))
                    .text_color(t.accent)
                    .font_weight(w(700.))
                    .child("z"),
            )
            .child(div().flex_1().child("doze itself")),
    )
}

/// A checkbox, ticked or not.
fn tick(t: &Tokens, on: bool) -> impl IntoElement {
    div()
        .size(px(14.))
        .flex_none()
        .rounded(px(4.))
        .border_1()
        .border_color(if on { t.accent } else { t.line2 })
        .bg(if on { t.accent } else { t.well })
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(10.))
        .font_weight(w(700.))
        .text_color(t.ink)
        .when(on, |d| d.child("✓"))
}
