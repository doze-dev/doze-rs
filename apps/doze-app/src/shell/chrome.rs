//! The window's frame around a page: the client-drawn title bar (tabs, the bell, help, ⌘K), the stack's head (name,
//! folder, the view switch), the vitals strip and the keys bar. Every size is the one shell.css gave it; every key
//! chip comes from the action table, so a chip always names a bound key.

use gpui::{AnyElement, IntoElement, SharedString, div, prelude::*, px, relative};

use crate::actions::{self, Spec};
use crate::model::Page;
use crate::theme::Tokens;
use crate::widgets::{icon, kbd_chip, kbd_plain, seg, switch, tag, w};

pub(super) fn text(s: &str) -> SharedString {
    SharedString::from(s.to_string())
}

fn spec(label: &str) -> Option<&'static Spec> {
    actions::all().find(|s| s.label == label)
}

pub(super) fn chip_of(label: &str) -> String {
    spec(label).and_then(actions::chip).unwrap_or_default()
}

/// The stack's head: name, folder and how long it is up; the views (only the Map is built yet, the rest say so);
/// Activity, All logs, the Up switch.
pub(crate) fn head(t: &Tokens, page: &Page) -> impl IntoElement {
    let views = views(t);
    let path = if page.up.is_empty() {
        page.dir.clone()
    } else {
        format!("{} · {}", page.dir, page.up)
    };
    let quiet = |label: &str, key: &str| {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(30.))
            .px(px(12.))
            .text_size(px(12.))
            .font_weight(w(500.))
            .text_color(t.dim)
            .opacity(0.6)
            .child(text(label))
            .child(kbd_plain(key, 11., t.dim, 500.))
    };
    head_row(t, page, &path)
        .child(views)
        .child(quiet("Activity", &chip_of("Activity")))
        .child(quiet("All logs", &chip_of("All logs")))
        .child(up(t, page))
}

/// The views: only the Map is built yet; the rest are shown, quiet, so the switch keeps its shape.
fn views(t: &Tokens) -> impl IntoElement {
    let soon = |label: &str| {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .opacity(0.45)
            .child(text(label))
            .into_any_element()
    };
    seg(
        t,
        "views",
        11.,
        vec![
            (soon("Tiles"), false, None),
            (soon("List"), false, None),
            (div().child("Map").into_any_element(), true, None),
            (
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .opacity(0.45)
                    .child("Grid")
                    .child(tag(t, "new"))
                    .into_any_element(),
                false,
                None,
            ),
        ],
    )
}

/// The head's name and folder.
fn head_row(t: &Tokens, page: &Page, path: &str) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .px(px(28.))
        .pt(px(22.))
        .pb(px(18.))
        .min_h(px(58.))
        .child(
            div()
                .min_w(px(72.))
                .text_size(px(20.))
                .font_weight(w(650.))
                .text_color(t.bright)
                .whitespace_nowrap()
                .child(text(&page.name)),
        )
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .text_color(t.dim)
                .whitespace_nowrap()
                .child(text(path)),
        )
        .child(div().flex_1())
}

/// The Up switch and the stack's menu.
fn up(t: &Tokens, page: &Page) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .child(div().font_weight(w(600.)).text_color(t.bright).child("Up"))
        .child(switch(t, page.running, true, false))
        .child(
            div()
                .w(px(28.))
                .h(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .child(icon("icons/dots.svg", 16., t.dim)),
        )
}

fn stat(t: &Tokens, value: &str, unit: &str, sub: AnyElement, wide: bool) -> gpui::Div {
    let s = div().flex().flex_col().gap(px(6.)).overflow_hidden().child(
        div()
            .flex()
            .items_start()
            .h(px(30.))
            .text_size(px(22.))
            .line_height(px(26.))
            .font_weight(w(600.))
            .text_color(t.bright)
            .child(text(value))
            .child(
                div()
                    .relative()
                    .top(px(3.))
                    .ml(px(6.))
                    .text_size(px(12.))
                    .font_weight(w(400.))
                    .text_color(t.dim)
                    .child(text(unit)),
            ),
    );
    let s = if wide { s.flex_1() } else { s.flex_none() };
    s.child(sub)
}

fn sub(t: &Tokens, s: &str) -> AnyElement {
    div()
        .text_size(px(12.))
        .text_color(t.dim)
        .whitespace_nowrap()
        .child(text(s))
        .into_any_element()
}

/// The memory figure's share bar and the biggest few.
fn memory(t: &Tokens, v: &crate::model::Vitals) -> AnyElement {
    let colors = [t.cool, t.accent, t.good, t.warn, t.extra, t.dim];
    let bar = div()
        .flex()
        .w(px(220.))
        .h(px(6.))
        .rounded(px(4.))
        .bg(t.track)
        .overflow_hidden()
        .children(
            v.bar
                .iter()
                .zip(colors.iter().cycle())
                .map(|(share, c)| div().h_full().w(relative(share.max(0.01))).bg(*c)),
        );
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(bar)
        .child(sub(t, &v.shares))
        .into_any_element()
}

/// Dozing and in trouble, under the awake figure.
fn states(t: &Tokens, v: &crate::model::Vitals) -> AnyElement {
    let mut counts: Vec<AnyElement> = Vec::new();
    for (n, word, color) in [
        (v.dozing, "dozing", t.warn),
        (v.trouble, "in trouble", t.bad),
    ] {
        if n > 0 {
            if !counts.is_empty() {
                counts.push(div().child(" · ").into_any_element());
            }
            counts.push(
                div()
                    .text_color(color)
                    .child(text(&format!("{n} {word}")))
                    .into_any_element(),
            );
        }
    }
    div()
        .flex()
        .text_size(px(12.))
        .text_color(t.dim)
        .whitespace_nowrap()
        .children(counts)
        .into_any_element()
}

/// The vitals: memory with its share bar, cores, how many are awake, connections.
pub(crate) fn strip(t: &Tokens, page: &Page) -> impl IntoElement {
    let v = &page.vitals;
    let (mem_sub, state_sub) = (memory(t, v), states(t, v));
    let (cores, unit) = v
        .cores
        .split_once(' ')
        .unwrap_or((v.cores.as_str(), "cores"));
    let conn_unit = if v.conns == 1 {
        "connection"
    } else {
        "connections"
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(36.))
        .mx(px(28.))
        .mb(px(18.))
        .pb(px(18.))
        .border_b_1()
        .border_color(t.line)
        .child(stat(t, &v.mem, "resident", mem_sub, true))
        .child(stat(t, cores, unit, sub(t, ""), false))
        .child(stat(t, &v.awake.to_string(), "awake", state_sub, false))
        .child(stat(
            t,
            &v.conns.to_string(),
            conn_unit,
            sub(t, &v.tasks),
            false,
        ))
}

/// The keys bar: the Map's keys, each chip from the action table, and what a click does.
pub(crate) fn keys_bar(t: &Tokens, lit: bool) -> impl IntoElement {
    let arrows = ["Card above", "Card below", "Card left", "Card right"]
        .iter()
        .map(|l| chip_of(l))
        .collect::<String>();
    let mut keys = vec![(arrows, "move")];
    for (label, what) in [
        ("Find a card", "find"),
        ("Fit the map", "fit"),
        ("Overview on or off", "overview"),
        ("Keys and words", "keys"),
        ("Search or do…", "do"),
    ] {
        keys.push((chip_of(label), what));
    }
    keys.push((chip_of("Close"), if lit { "let go" } else { "close" }));
    let note = if lit {
        "selected: its calls stay lit, the rest steps back"
    } else {
        "hover a card to see its calls · click to keep them"
    };
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
        .children(keys.into_iter().map(|(k, what)| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(5.))
                .child(kbd_chip(t, &k, 11., 600., 6.))
                .child(what)
        }))
        .child(div().flex_1())
        .child(div().child(note))
}
