//! The Tiles view (the Tiles board): a tile per service under its group, four across (three, then two, as the window
//! narrows), each with its dot, name, pin and switch, its word and why, its memory and address. Also the row parts the
//! List shares: the dot, the switch, the word's tone, the filter box.

use doze_desk::overview::Row;
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};
use gpui_component::input::Input;

use super::{M2, OverviewView};
use crate::kit::{chip_of, s, tone, word_line};
use crate::theme::{Tokens, mix};
use crate::widgets::{MONO_FAMILY, dot, icon, kbd_chip, switch, tracked, w};

/// Tiles to a row at the window's width.
pub(super) fn per_row(v: &OverviewView) -> usize {
    match v.width {
        w if w <= 860. => 2,
        w if w <= 1100. => 3,
        _ => 4,
    }
}

/// The row's dot (`Dot`): a task's glyph, red in trouble, green awake, yellow dozing, a ring while waking.
pub(super) fn row_dot(t: &Tokens, r: &Row) -> AnyElement {
    if r.kind == "task" {
        return div()
            .w(px(8.))
            .text_size(px(12.))
            .text_color(tone(t, r.look.tone))
            .child(r.look.glyph)
            .into_any_element();
    }
    if r.trouble {
        return dot(t.bad).into_any_element();
    }
    match r.look.word {
        "awake" => dot(t.good).into_any_element(),
        "dozing" => dot(t.warn).into_any_element(),
        "waking" => div()
            .size(px(8.))
            .flex_none()
            .rounded_full()
            .border_2()
            .border_color(t.cool)
            .into_any_element(),
        _ => dot(t.dim).into_any_element(),
    }
}

/// The word's tone (`wordTone`).
pub(super) fn word_tone(t: &Tokens, r: &Row) -> gpui::Hsla {
    if r.trouble {
        return t.bad;
    }
    match r.look.word {
        "awake" | "done" => t.good,
        "dozing" => t.warn,
        "waking" => t.cool,
        "ready" => t.accent,
        "failed" => t.bad,
        _ => t.text,
    }
}

/// The row's switch (`Switch`): Run for a task, ⊘ when paused in doze.hcl, else the switch; off for a down row.
pub(super) fn row_switch(t: &Tokens, r: &Row, cx: &mut Context<OverviewView>) -> AnyElement {
    let say = cx.listener(|this, _, _, cx| this.say(M2, cx));
    let id = gpui::SharedString::from(format!("sw-{}", r.name));
    if r.kind == "task" {
        if r.down {
            return div().into_any_element();
        }
        let label = if matches!(r.look.word, "done" | "failed") {
            "Run again"
        } else {
            "Run"
        };
        return div()
            .id(id)
            .flex_none()
            .h(px(24.))
            .px(px(10.))
            .rounded(px(6.))
            .border_1()
            .border_color(t.accent)
            .flex()
            .items_center()
            .text_size(px(11.))
            .font_weight(w(600.))
            .text_color(t.accent)
            .hover(|d| d.bg(t.sel))
            .child(label)
            .on_click(say)
            .into_any_element();
    }
    if r.paused {
        return div()
            .flex_none()
            .text_color(t.dim)
            .child("⊘")
            .into_any_element();
    }
    div()
        .id(id)
        .flex_none()
        .child(switch(t, r.on, false, r.off || r.frozen))
        .on_click(say)
        .into_any_element()
}

/// `.toolbar .filter`: the filter box and its key.
pub(super) fn filter(t: &Tokens, v: &OverviewView) -> impl IntoElement {
    div().flex().flex_none().px(px(28.)).pb(px(14.)).child(
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(28.))
            .w(px(260.))
            .pl(px(2.))
            .pr(px(8.))
            .rounded(px(7.))
            .border_1()
            .border_color(t.line2)
            .bg(t.well)
            .child(
                div()
                    .flex_1()
                    .child(Input::new(&v.filter).appearance(false).text_size(px(12.))),
            )
            .child(kbd_chip(t, &chip_of("Filter"), 11., 600., 5.)),
    )
}

/// A group's heading over its tiles (`.gl`): the title and what it is.
fn group_head(t: &Tokens, title: &str, what: &str) -> impl IntoElement {
    let title = format!(
        "{}{}",
        title.get(..1).unwrap_or(""),
        title.get(1..).unwrap_or("").to_lowercase()
    );
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .mx(px(28.))
        .mb(px(12.))
        .child(tracked(&title, 11., 700., t.dim, 1.1))
        .child(
            div()
                .text_size(px(11.))
                .text_color(t.dim)
                .opacity(0.8)
                .child(s(what)),
        )
}

/// The tile's top line: dot, name, pin, switch.
fn tile_top(t: &Tokens, r: &Row, off: bool, cx: &mut Context<OverviewView>) -> impl IntoElement {
    let name = div()
        .text_size(px(15.))
        .font_weight(w(650.))
        .text_color(if off { t.dim } else { t.bright })
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .child(s(&r.name));
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .min_w_0()
        .child(row_dot(t, r))
        .child(name)
        .when(r.pinned, |d| d.child(icon("icons/pin.svg", 13., t.accent)))
        .child(div().flex_1())
        .child(row_switch(t, r, cx))
}

/// The word, in its tone, and why.
fn tile_state(t: &Tokens, r: &Row) -> impl IntoElement {
    div()
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .text_color(t.dim)
        .child(word_line(r.look.word, word_tone(t, r), &[&r.because]))
}

/// The foot: memory and the address with its copy mark; or, in trouble, what it last said.
fn tile_foot(t: &Tokens, r: &Row) -> gpui::Div {
    if !r.err.is_empty() {
        return div()
            .mt_auto()
            .font_family(MONO_FAMILY)
            .text_size(px(11.5))
            .text_color(t.bad)
            .whitespace_nowrap()
            .overflow_hidden()
            .text_ellipsis()
            .child(s(&r.err));
    }
    let f = div()
        .flex()
        .items_center()
        .gap(px(10.))
        .mt_auto()
        .text_size(px(12.5))
        .text_color(t.dim)
        .whitespace_nowrap()
        .min_w_0()
        .when(!r.mem.is_empty(), |d| {
            d.child(div().flex_none().text_color(t.text).child(s(&r.mem)))
        });
    if r.kind == "task" {
        return f.when(r.engine != "task", |d| {
            d.child(div().font_family(MONO_FAMILY).child(s(&r.engine)))
        });
    }
    let link = r.awake && !r.url.is_empty();
    f.when(!r.address.is_empty(), |d| {
        d.child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .when(link, |d| d.underline())
                .child(s(&r.address)),
        )
        .child(icon("icons/copy.svg", 12., t.dim))
    })
}

/// One tile.
fn tile(t: &Tokens, r: &Row, selected: bool, cx: &mut Context<OverviewView>) -> impl IntoElement {
    let off = !r.on && r.kind != "task";
    let (border, bg) = if r.trouble {
        (mix(t.bad, t.line, 55.), mix(t.bad, t.panel, 7.))
    } else if selected {
        (t.line2, mix(t.accent, t.panel, 7.))
    } else if off {
        (t.line, gpui::transparent_black())
    } else {
        (t.line, t.panel)
    };
    let name = r.name.clone();
    div()
        .id(gpui::SharedString::from(format!("tile-{}", r.name)))
        .flex()
        .flex_col()
        .gap(px(8.))
        .flex_1()
        .min_w_0()
        .min_h(px(112.))
        .pt(px(16.))
        .px(px(18.))
        .pb(px(14.))
        .rounded(px(12.))
        .border_1()
        .border_color(border)
        .bg(bg)
        .when(off && !r.trouble && !selected, |d| d.border_dashed())
        .hover(|d| d.border_color(t.line2))
        .child(tile_top(t, r, off, cx))
        .child(tile_state(t, r))
        .child(tile_foot(t, r))
        .on_click(cx.listener(move |this, _, _, cx| this.select(&name, cx)))
}

/// The view: the filter, then each group's tiles.
pub(super) fn view(
    t: &Tokens,
    v: &OverviewView,
    cx: &mut Context<OverviewView>,
) -> impl IntoElement {
    let per = per_row(v);
    let shown: Vec<&str> = v.shown().iter().map(|r| r.name.as_str()).collect();
    let mut body = div()
        .id("tiles")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll();
    if v.home.groups.is_empty() && v.home.stopped.is_none() {
        body = body.child(div().mx(px(28.)).text_color(t.dim).child(
            "doze.hcl declares nothing yet: add an app or a module to it, and it shows here.",
        ));
    }
    for g in &v.home.groups {
        let rows: Vec<&Row> = g
            .rows
            .iter()
            .filter(|r| shown.contains(&r.name.as_str()))
            .collect();
        if rows.is_empty() {
            continue;
        }
        body = body.child(group_head(t, g.title, g.what));
        let mut grid = div().flex().flex_col().gap(px(14.)).mx(px(28.)).mb(px(26.));
        for chunk in rows.chunks(per) {
            let mut line = div().flex().gap(px(14.));
            for r in chunk {
                line = line.child(tile(
                    t,
                    r,
                    v.selected.as_deref() == Some(r.name.as_str()),
                    cx,
                ));
            }
            for _ in chunk.len()..per {
                // The empty places are framed as a tile is (padding and border), so every tile is the same width.
                line = line.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .px(px(18.))
                        .border_1()
                        .border_color(gpui::transparent_black()),
                );
            }
            grid = grid.child(line);
        }
        body = body.child(grid);
    }
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .child(filter(t, v))
        .child(body)
}
