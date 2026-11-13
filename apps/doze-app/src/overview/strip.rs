//! The strip of figures under the head (`.strip`): memory with its share bar, cores, the instances by state,
//! connections and tasks; for a stopped stack, the strip that says what Up starts, or that doze.hcl has a mistake, with
//! the mistake shown where it is.

use doze_desk::overview::{ConfigError, Stopped, Vitals4};
use gpui::{AnyElement, IntoElement, div, prelude::*, px, relative};

use super::{OverviewView, View};
use crate::kit::{Btn, btn, mono, s};
use crate::theme::{Tokens, mix};
use crate::widgets::{kbd_plain, w};

/// The strip: the four figures for a running stack, the down strip for a stopped one.
pub(super) fn strip(t: &Tokens, v: &OverviewView, width: f32) -> AnyElement {
    let fit = matches!(v.view, View::Map | View::Grid);
    let tight = v.view == View::List;
    match &v.home.stopped {
        Some(st) => {
            let mut col = div()
                .flex()
                .flex_col()
                .flex_none()
                .child(down_strip(t, st, tight));
            if let Some(c) = &st.config {
                col = col.child(source_card(t, c));
            }
            col.into_any_element()
        }
        None => figures(
            t,
            &v.home.vitals,
            width < 960.,
            tight,
            fit,
            !v.home.banner.ok,
        )
        .into_any_element(),
    }
}

fn strip_box(t: &Tokens, tight: bool, fit: bool) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(36.))
        .mx(px(28.))
        .mb(px(if tight {
            0.
        } else if fit {
            18.
        } else {
            28.
        }))
        .pb(px(if fit { 18. } else { 22. }))
        .border_b_1()
        .border_color(t.line)
}

fn stat(t: &Tokens, value: &str, unit: &str, under: AnyElement, wide: bool) -> gpui::Div {
    let v = div()
        .flex()
        .items_baseline()
        .whitespace_nowrap()
        .child(
            div()
                .text_size(px(22.))
                .line_height(px(26.))
                .font_weight(w(600.))
                .text_color(t.bright)
                .child(s(value)),
        )
        .child(
            div()
                .ml(px(6.))
                .text_size(px(12.))
                .text_color(t.dim)
                .child(s(unit)),
        );
    let st = div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .min_w_0()
        .overflow_hidden()
        .child(v)
        .child(under);
    if wide { st.flex_1() } else { st.flex_none() }
}

fn under(t: &Tokens, text: &str) -> AnyElement {
    div()
        .text_size(px(12.))
        .text_color(t.dim)
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .child(s(text))
        .into_any_element()
}

/// `.strip`: memory with its share bar, cores, how many awake (and the rest by state), connections and tasks.
fn figures(
    t: &Tokens,
    v: &Vitals4,
    narrow: bool,
    tight: bool,
    fit: bool,
    stale: bool,
) -> impl IntoElement {
    let cores = v
        .cpu
        .total
        .split_whitespace()
        .next()
        .unwrap_or("0")
        .to_owned();
    let unit = if v.conns.open == 1 {
        "connection"
    } else {
        "connections"
    };
    let tasks = if v.conns.tasks.is_empty() {
        "through doze"
    } else {
        v.conns.tasks.as_str()
    };
    let mut row = strip_box(t, tight, fit)
        .when(narrow, |d| d.gap(px(20.)))
        .child(stat(t, &v.mem.total, "resident", memory(t, v), true));
    if !narrow {
        // The narrow board drops the cores figure.
        row = row.child(stat(t, &cores, "cores", under(t, &v.cpu.key), false));
    }
    row.child(stat(
        t,
        &v.state.awake.to_string(),
        "awake",
        states(t, v),
        false,
    ))
    .child(stat(
        t,
        &v.conns.open.to_string(),
        unit,
        under(t, tasks),
        false,
    ))
    .when(stale, |d| d.opacity(0.5))
}

/// The memory figure's share bar (each instance its colour, biggest first) and the biggest few.
fn memory(t: &Tokens, v: &Vitals4) -> AnyElement {
    let colors = [t.cool, t.accent, t.good, t.warn, t.extra, t.dim];
    let parts = v.mem.shares.iter().zip(colors.iter().cycle());
    let bar = div()
        .flex()
        .w(px(220.))
        .max_w_full()
        .h(px(6.))
        .rounded(px(4.))
        .bg(t.track)
        .overflow_hidden()
        .children(parts.map(|(sh, c)| {
            div()
                .h_full()
                .w(relative((sh.part as f32).max(0.01)))
                .bg(*c)
        }));
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(bar)
        .child(under(t, &v.mem.key))
        .into_any_element()
}

/// Under the awake figure: the rest by state, each in its tone.
fn states(t: &Tokens, v: &Vitals4) -> AnyElement {
    let s8 = &v.state;
    let mut counts: Vec<AnyElement> = Vec::new();
    let all = [
        (s8.dozing, "dozing", t.warn),
        (s8.waking, "waking", t.cool),
        (s8.asleep, "asleep", t.dim),
        (s8.off, "off", t.dim),
        (s8.crashing, "crashing", t.bad),
    ];
    for (n, word, c) in all.into_iter().filter(|(n, _, _)| *n > 0) {
        if !counts.is_empty() {
            counts.push(div().child(" · ").into_any_element());
        }
        counts.push(
            div()
                .text_color(c)
                .child(s(&format!("{n} {word}")))
                .into_any_element(),
        );
    }
    if counts.is_empty() {
        counts.push(div().child("everything awake").into_any_element());
    }
    div()
        .flex()
        .text_size(px(12.))
        .text_color(t.dim)
        .whitespace_nowrap()
        .children(counts)
        .into_any_element()
}

/// The strip of a stopped stack: that it is down, and what the switches start; or that doze.hcl has a mistake.
fn down_strip(t: &Tokens, st: &Stopped, tight: bool) -> impl IntoElement {
    let row = strip_box(t, tight, false);
    if !st.problem.is_empty() {
        let v = div()
            .flex()
            .items_baseline()
            .child(
                div()
                    .text_size(px(22.))
                    .font_weight(w(600.))
                    .text_color(t.bad)
                    .child("doze.hcl has a mistake"),
            )
            .child(
                div()
                    .ml(px(6.))
                    .text_size(px(12.))
                    .text_color(t.dim)
                    .child("nothing can start until it is fixed"),
            );
        return row
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .flex_1()
                    .min_w_0()
                    .child(v)
                    .child(under(t, &st.problem)),
            )
            .child(btn(
                t,
                "edit-hcl",
                "Edit doze.hcl",
                "⌘E",
                Btn::Primary,
                None,
            ));
    }
    let v = div()
        .flex()
        .items_baseline()
        .child(
            div()
                .text_size(px(22.))
                .font_weight(w(600.))
                .text_color(t.bright)
                .child("Stopped"),
        )
        .child(
            div()
                .ml(px(6.))
                .text_size(px(12.))
                .text_color(t.dim)
                .child(s(&st.when)),
        );
    let line = format!(
        "{} A service's own switch starts just it and what it needs. Modules wake on their own when something connects.",
        st.last_line
    );
    row.child(
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .flex_1()
            .min_w_0()
            .child(v)
            .child(under(t, &line)),
    )
}

/// `.srccard`: the mistake where it is: the lines around it, what is wrong, what to write.
fn source_card(t: &Tokens, c: &ConfigError) -> impl IntoElement {
    let name = std::path::Path::new(&c.file)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "doze.hcl".into());
    let line = div()
        .font_weight(w(400.))
        .child(s(&format!("· line {}", c.line)));
    let title = div()
        .flex()
        .gap(px(6.))
        .text_size(px(12.))
        .font_weight(w(600.))
        .text_color(t.dim)
        .child(s(&name.to_uppercase()))
        .when(c.line > 0, |d| d.child(line));
    let summary = div()
        .font_weight(w(600.))
        .text_color(t.bright)
        .child(s(&c.summary));
    let detail = div().text_color(t.dim).child(s(&format!("· {}", c.detail)));
    let fix = div()
        .flex()
        .gap(px(6.))
        .child(div().text_color(t.dim).child("To fix it"))
        .child(s(&c.hint));
    let lint = div()
        .flex()
        .gap(px(4.))
        .text_size(px(12.))
        .text_color(t.dim)
        .child("The same message as")
        .child(kbd_plain("doze lint", 12., t.dim, 400.))
        .child(".");
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .mx(px(28.))
        .my(px(14.))
        .px(px(16.))
        .py(px(14.))
        .rounded(px(10.))
        .border_1()
        .border_color(t.line)
        .bg(t.panel)
        .child(title)
        .when(!c.excerpt.is_empty(), |d| d.child(excerpt(t, c)))
        .child(
            div()
                .flex()
                .gap(px(6.))
                .child(summary)
                .when(!c.detail.is_empty(), |d| d.child(detail)),
        )
        .when(!c.hint.is_empty(), |d| d.child(fix))
        .child(lint)
}

/// The lines around the mistake, the bad one marked.
fn excerpt(t: &Tokens, c: &ConfigError) -> impl IntoElement {
    let lines = c.excerpt.iter().map(|l| {
        let text = s(if l.text.is_empty() { " " } else { &l.text });
        div()
            .flex()
            .gap(px(12.))
            .when(l.bad, |d| d.bg(mix(t.bad, t.well, 18.)))
            .child(mono(s(&l.n.to_string()), 12., t.dim).w(px(24.)).flex_none())
            .child(mono(text, 12., if l.bad { t.bright } else { t.text }))
    });
    div()
        .flex()
        .flex_col()
        .px(px(10.))
        .py(px(8.))
        .rounded(px(7.))
        .bg(t.well)
        .children(lines)
}
