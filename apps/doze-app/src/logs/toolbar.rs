//! The logs page around its lines, as the InstanceLogs and AllLogs boards lay it out: the head (All logs only), the
//! toolbar (the filter, the level, the run, the time; the counts and Copy at the right), the dock with its cap line
//! (following or paused), and the keys.

use doze_desk::page::logs::LEVEL_TABS;
use gpui::{AnyElement, Context, Entity, IntoElement, div, prelude::*, px};
use gpui_component::Sizable as _;
use gpui_component::input::{Input, InputState};

use super::{LogsMode, LogsView, list};
use crate::instance::head::{chip, keys_bar};
use crate::instance::ui::{button, text};
use crate::theme::{Tokens, ty};
use crate::widgets::{OnClick, kbd_chip, seg};

/// All logs' head: the title, following or paused, and Pause / Follow.
pub(crate) fn head(t: &Tokens, v: &LogsView, cx: &mut Context<LogsView>) -> impl IntoElement {
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
                .text_size(px(ty::XL))
                .font_weight(ty::HEADING)
                .text_color(t.bright)
                .child("All logs"),
        )
        .child(div().text_color(t.dim).child(text(&format!(
            "every service, merged · {}",
            if v.follow { "following" } else { "paused" }
        ))))
        .child(div().flex_1())
        .child(
            button(
                t,
                "pause",
                if v.follow { "Pause" } else { "Follow" },
                &chip("Pause or follow"),
                false,
            )
            .on_click(cx.listener(|v, _, _, cx| v.set_follow(!v.follow, cx))),
        )
}

fn click(
    cx: &mut Context<LogsView>,
    f: impl Fn(&mut LogsView, &mut Context<LogsView>) + 'static,
) -> Option<OnClick> {
    Some(Box::new(cx.listener(move |v, _, _, cx| f(v, cx))))
}

/// The filter box, the level, the run and the time; the counts and Copy.
pub(crate) fn toolbar(
    t: &Tokens,
    v: &LogsView,
    input: Entity<InputState>,
    cx: &mut Context<LogsView>,
) -> impl IntoElement {
    let total = v.index.len();
    let filter = filter_box(t, v, input, cx);
    let levels = level_seg(t, v, cx);
    let mut bar = div()
        .flex()
        .flex_none()
        .flex_wrap()
        .items_center()
        .gap(px(10.))
        .px(px(28.))
        .pt(px(if v.mode == LogsMode::All { 0. } else { 12. }))
        .pb(px(12.))
        .child(filter)
        .child(levels);
    if let Some(runs) = runs_seg(t, v, cx) {
        bar = bar.child(runs);
    }
    bar.child(time_seg(t, v, cx))
        .child(div().flex_1())
        .child(counts(t, v))
        .when(total > 0, |d| {
            d.child(
                button(
                    t,
                    "copy-lines",
                    if v.picked.is_some() {
                        "Copy picked"
                    } else {
                        "Copy"
                    },
                    &chip("Copy the lines"),
                    false,
                )
                .on_click(cx.listener(|v, _, _, cx| v.copy_shown(cx))),
            )
        })
}

/// The filter box: the input, what the filter found, the find toggle and its key.
fn filter_box(
    t: &Tokens,
    v: &LogsView,
    input: Entity<InputState>,
    cx: &mut Context<LogsView>,
) -> gpui::Div {
    let total = v.index.len();
    let typed = !v.filter_text.is_empty();
    let said = match (typed, v.index.query().filter.error()) {
        (_, Some(e)) => Some((format!("not a pattern: {e}"), t.bad)),
        (true, None) if v.find => Some(match v.hit {
            Some((_, at)) => (format!("{} of {}", at + 1, v.index.hit_count()), t.dim),
            None => (format!("{} found", v.index.hit_count()), t.dim),
        }),
        (true, None) => Some((format!("{} of {total}", v.index.matches()), t.dim)),
        _ => None,
    };
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(28.))
        .pl(px(4.))
        .pr(px(6.))
        .w(px(300.))
        .flex_shrink(1.)
        .rounded(px(7.))
        .border_1()
        .border_color(t.line2)
        .bg(t.well)
        .text_size(px(ty::SM))
        .child(
            div().flex_1().min_w_0().child(
                Input::new(&input)
                    .appearance(false)
                    .cleanable(true)
                    .with_size(gpui_component::Size::Small),
            ),
        )
        .children(said.map(|(s, c)| {
            div()
                .flex_none()
                .whitespace_nowrap()
                .text_color(c)
                .child(text(&s))
        }))
        .child(find_toggle(t, v, cx))
        .child(kbd_chip(
            t,
            &chip("Filter the lines"),
            ty::XS,
            ty::SEMIBOLD,
            5.,
        ))
}

/// The level: one segment per level tab.
fn level_seg(t: &Tokens, v: &LogsView, cx: &mut Context<LogsView>) -> AnyElement {
    seg(
        t,
        "level",
        11.,
        LEVEL_TABS
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let l = i as u8;
                (
                    div().child(*name).into_any_element(),
                    v.level == l,
                    click(cx, move |v, cx| v.set_level(l, cx)),
                )
            })
            .collect(),
    )
    .into_any_element()
}

/// The time: Relative or Clock.
fn time_seg(t: &Tokens, v: &LogsView, cx: &mut Context<LogsView>) -> AnyElement {
    seg(
        t,
        "time",
        11.,
        vec![
            (
                div().child("Relative").into_any_element(),
                !v.clock,
                click(cx, |v, cx| {
                    v.clock = false;
                    cx.notify()
                }),
            ),
            (
                div().child("Clock").into_any_element(),
                v.clock,
                click(cx, |v, cx| {
                    v.clock = true;
                    cx.notify()
                }),
            ),
        ],
    )
    .into_any_element()
}

/// This run / All N runs / run k of N, when there is more than one run.
fn runs_seg(t: &Tokens, v: &LogsView, cx: &mut Context<LogsView>) -> Option<AnyElement> {
    let n = v.index.runs().len();
    if n < 2 {
        return None;
    }
    let mut items = vec![
        (
            div().child("This run").into_any_element(),
            v.run_back == 1,
            click(cx, |v, cx| v.set_run_back(1, cx)),
        ),
        (
            div()
                .child(text(&format!("All {n} runs")))
                .into_any_element(),
            v.run_back == 0,
            click(cx, |v, cx| v.set_run_back(0, cx)),
        ),
    ];
    if v.run_back > 1 {
        items.push((
            div()
                .child(text(&format!("run {} of {n}", n + 1 - v.run_back)))
                .into_any_element(),
            true,
            None,
        ));
    }
    Some(seg(t, "runs", 11., items).into_any_element())
}

/// `3 errors · 12 warnings · 4,210 lines kept`.
fn counts(t: &Tokens, v: &LogsView) -> impl IntoElement {
    let [_, _, warns, errors] = v.index.counts();
    let mut d = div()
        .flex()
        .flex_none()
        .whitespace_nowrap()
        .text_size(px(ty::SM))
        .text_color(t.dim);
    if errors > 0 {
        d = d.child(div().text_color(t.bad).child(text(&format!(
            "{errors} error{} · ",
            if errors == 1 { "" } else { "s" }
        ))));
    }
    if warns > 0 {
        d = d.child(div().text_color(t.warn).child(text(&format!(
            "{warns} warning{} · ",
            if warns == 1 { "" } else { "s" }
        ))));
    }
    d.child(text(&format!(
        "{} line{} kept",
        doze_display::thousands(v.index.len() as i64),
        if v.index.len() == 1 { "" } else { "s" }
    )))
}

/// The dock: the lines, and the cap line under them.
pub(crate) fn dock(t: &Tokens, v: &mut LogsView, cx: &mut Context<LogsView>) -> impl IntoElement {
    let cap = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.))
        .h(px(32.))
        .px(px(12.))
        .border_t_1()
        .border_color(t.line)
        .text_size(px(ty::SM))
        .text_color(t.dim)
        .child(text(if v.follow {
            "▼ following · new lines appear at the end"
        } else {
            "paused · scrolled away from the end"
        }))
        .child(div().flex_1())
        .when(!v.follow, |d| {
            d.child(
                crate::instance::ui::copy_link(t, "follow-again", "Follow again")
                    .on_click(cx.listener(|v, _, _, cx| v.set_follow(true, cx))),
            )
        });
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .mx(px(28.))
        .mb(px(3.))
        .rounded(px(10.))
        .border_1()
        .border_color(t.line)
        .bg(t.well)
        .overflow_hidden()
        .child(list::list(t, v, cx))
        .child(cap)
}

/// The logs' keys.
pub(crate) fn keys(t: &Tokens, v: &LogsView) -> impl IntoElement {
    let mut k: Vec<(String, &str)> = vec![
        (
            chip("Close"),
            if v.mode == LogsMode::All {
                "overview"
            } else {
                "up"
            },
        ),
        (
            chip("Pause or follow"),
            if v.follow { "pause" } else { "follow" },
        ),
        (chip("Filter the lines"), "filter"),
        ("1-4".to_owned(), "level"),
        (chip("Relative or clock"), "relative / clock"),
        (
            format!("{} {}", chip("Run before"), chip("Run after")),
            "run",
        ),
        (chip("Copy the lines"), "copy"),
        (chip("Find or filter"), "find"),
    ];
    if v.find {
        k.push((
            format!("{} {}", chip("Next match"), chip("Match before")),
            "next / before",
        ));
    }
    keys_bar(
        t,
        k,
        if v.mode == LogsMode::All {
            "from Overview: All logs, or ⇧L"
        } else {
            "click a line to pick it, ⇧-click for more"
        },
    )
}

/// Find inside the filter box: keep every line and step through the matches instead of hiding the rest.
fn find_toggle(t: &Tokens, v: &LogsView, cx: &mut Context<LogsView>) -> impl IntoElement {
    div()
        .id("find-toggle")
        .flex_none()
        .px(px(6.))
        .h(px(18.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .text_size(px(ty::XS))
        .font_weight(ty::SEMIBOLD)
        .cursor_pointer()
        .when(v.find, |d| d.bg(t.accent).text_color(t.ink))
        .when(!v.find, |d| {
            d.text_color(t.dim).hover(|s| s.text_color(t.bright))
        })
        .on_click(cx.listener(|v, _, _, cx| {
            v.find = !v.find;
            v.rebuild(cx);
        }))
        .child("find")
}
