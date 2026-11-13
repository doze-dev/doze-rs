//! The page's head (the state, the name, the path line, the switch and the verbs that fit the state), its tabs, and
//! its keys bar. Every chip names a key the action table binds.

use doze_desk::page::instance::tab_title;
use doze_display::{Look, TAB_LOGS};
use gpui::{AnyElement, Context, IntoElement, SharedString, div, prelude::*, px};

use super::InstanceView;
use super::ui::{self, button, state_mark, text};
use crate::actions;
use crate::theme::{Tokens, ty};
use crate::widgets::{kbd_chip, switch};

/// The first key of the table's row `label`, as a chip writes it.
pub(crate) fn chip(label: &str) -> String {
    actions::all()
        .find(|s| s.label == label)
        .and_then(actions::chip)
        .unwrap_or_default()
}

pub(crate) fn head(
    t: &Tokens,
    v: &InstanceView,
    cx: &mut Context<InstanceView>,
) -> impl IntoElement {
    let p = &v.page;
    let trouble = p.crash.is_some();
    // The sidebar's mark, larger: a running task's ring is the same blue ring there and here.
    let mark = if p.is_task() {
        state_mark(t, Some(p.look.unwrap_or(Look::NotRun)), false, 10.)
    } else {
        state_mark(t, p.look, trouble, 10.)
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .px(px(28.))
        .pt(px(22.))
        .pb(px(18.))
        .min_h(px(58.))
        .child(mark)
        .child(
            div()
                .flex_none()
                .min_w(px(72.))
                .max_w(px(320.))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(px(ty::XL))
                .font_weight(ty::HEADING)
                .text_color(t.bright)
                .child(text(&v.name)),
        )
        .child(
            div()
                .flex_initial()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_color(t.dim)
                .child(text(&p.path)),
        )
        .child(div().flex_1())
        .children(verbs(t, v, cx))
}

type Act = fn(&mut InstanceView, &mut gpui::Window, &mut Context<InstanceView>);

/// A button that runs `f` on the page.
fn act(
    t: &Tokens,
    id: &'static str,
    (label, key): (&str, &str),
    primary: bool,
    f: Act,
    cx: &mut Context<InstanceView>,
) -> AnyElement {
    button(t, id, label, key, primary)
        .on_click(cx.listener(move |v, _, w, cx| f(v, w, cx)))
        .into_any_element()
}

/// The switch and the buttons, as the state allows.
fn verbs(t: &Tokens, v: &InstanceView, cx: &mut Context<InstanceView>) -> Vec<AnyElement> {
    let p = &v.page;
    if p.is_task() {
        let ran = !p.runs.is_empty();
        let first = if ran {
            act(
                t,
                "copy-output",
                ("Copy output", ""),
                false,
                |v, _, cx| v.copy_output(cx),
                cx,
            )
        } else {
            act(
                t,
                "edit",
                ("Edit doze.hcl", ""),
                false,
                |v, _, cx| v.edit_config(cx),
                cx,
            )
        };
        let run = run_label(p.look, ran);
        return vec![
            first,
            act(
                t,
                "run",
                (run, &chip("Run the task")),
                true,
                InstanceView::run_task,
                cx,
            ),
        ];
    }
    let mut out = vec![switch_el(t, v, cx)];
    if p.paused {
        out.push(act(
            t,
            "edit",
            ("Edit doze.hcl", ""),
            false,
            |v, _, cx| v.edit_config(cx),
            cx,
        ));
    } else if let Some(c) = &p.crash {
        out.extend(crash_verbs(t, c.gave_up, cx));
    } else {
        out.extend(service_verbs(t, v, cx));
    }
    out
}

/// A crashing one: stop trying, or try now. A failed one is not being tried again: nothing to stop, and Restart, as
/// Needs you says it.
fn crash_verbs(t: &Tokens, gave_up: bool, cx: &mut Context<InstanceView>) -> Vec<AnyElement> {
    let mut out = Vec::new();
    if !gave_up {
        out.push(act(
            t,
            "stop",
            ("Stop trying", &chip("Sleep / stop trying")),
            false,
            InstanceView::sleep,
            cx,
        ));
    }
    let label = if gave_up { "Restart" } else { "Try now" };
    out.push(act(
        t,
        "try",
        (label, &chip("Restart / try now")),
        true,
        InstanceView::restart,
        cx,
    ));
    out
}

/// The word and the switch (a lock when doze.hcl paused it).
fn switch_el(t: &Tokens, v: &InstanceView, cx: &mut Context<InstanceView>) -> AnyElement {
    let p = &v.page;
    let sw = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .text_size(px(ty::SM))
        .child(
            div()
                .text_color(v.word_color(cx))
                .child(p.look.map_or("", Look::word)),
        );
    if p.paused {
        return sw
            .child(div().text_color(t.dim).child("⊘"))
            .into_any_element();
    }
    sw.child(
        div()
            .id("switch")
            .cursor_pointer()
            .on_click(cx.listener(|v, _, w, cx| v.flip(w, cx)))
            .child(switch(t, &format!("inst/{}", p.name), p.on, false, p.off)),
    )
    .into_any_element()
}

/// A service's verbs: pin, restart, sleep, wake, open in the browser.
fn service_verbs(t: &Tokens, v: &InstanceView, cx: &mut Context<InstanceView>) -> Vec<AnyElement> {
    let p = &v.page;
    let (serving, url) = (p.serving(), p.url().map(str::to_owned));
    let mut out = Vec::new();
    if !p.off {
        let label = if p.pinned { "Unpin" } else { "Pin awake" };
        out.push(act(
            t,
            "pin",
            (label, &chip("Pin awake")),
            false,
            InstanceView::pin,
            cx,
        ));
    }
    if serving || !p.stuck.is_empty() {
        out.push(act(
            t,
            "restart",
            ("Restart", &chip("Restart / try now")),
            false,
            InstanceView::restart,
            cx,
        ));
    }
    if serving && url.is_none() {
        out.push(act(
            t,
            "sleep",
            ("Sleep", &chip("Sleep / stop trying")),
            false,
            InstanceView::sleep,
            cx,
        ));
    }
    if !p.on && !p.off {
        out.push(act(
            t,
            "wake",
            ("Wake", &chip("Wake")),
            true,
            InstanceView::flip,
            cx,
        ));
    }
    if let (Some(u), true) = (url, serving) {
        let shown = u
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .to_owned();
        out.push(
            button(t, "open", "Open", "", true)
                .child(
                    div()
                        .max_w(px(220.))
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_size(px(ty::SM))
                        .child(text(&shown)),
                )
                .child(
                    div()
                        .text_size(px(ty::XS))
                        .opacity(0.7)
                        .child(text(&chip("Open in browser"))),
                )
                .on_click(cx.listener(|v, _, _, cx| v.open_address(cx)))
                .into_any_element(),
        );
    }
    out
}

/// `.ptabs`: the page's tabs, the open one underlined in the accent.
pub(crate) fn tabs(
    t: &Tokens,
    v: &InstanceView,
    cx: &mut Context<InstanceView>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .gap(px(4.))
        .px(px(28.))
        .border_b_1()
        .border_color(t.line)
        .children(v.page.tabs.iter().enumerate().map(|(i, tab)| {
            let on = tab.id == v.tab;
            let id = tab.id.clone();
            div()
                .id(("ptab", i))
                .px(px(12.))
                .pt(px(9.))
                .pb(px(8.))
                .mb(px(-1.))
                .border_b_2()
                .border_color(if on {
                    t.accent
                } else {
                    gpui::transparent_black()
                })
                .text_color(if on { t.bright } else { t.dim })
                .whitespace_nowrap()
                .cursor_pointer()
                .hover(|s| s.text_color(t.text))
                .on_click(cx.listener(move |v, _, w, cx| v.set_tab(&id, w, cx)))
                .child(text(&tab_title(tab, &v.page)))
        }))
}

/// `.keys`: the page's keys, chips from the table.
pub(crate) fn keys(t: &Tokens, v: &InstanceView) -> impl IntoElement {
    let p = &v.page;
    let mut keys: Vec<(String, &str)> = Vec::new();
    if p.is_task() {
        keys.push((chip("Run the task"), "run"));
        keys.push((chip("Logs"), "runs"));
        keys.push((chip("Close"), "up"));
    } else {
        keys.push((chip("Close"), "up"));
        keys.push((chip("Logs"), "logs"));
        keys.push((chip("Copy address"), "copy address"));
        if p.url().is_some() && p.serving() {
            keys.push((chip("Open in browser"), "open in browser"));
        }
        if let Some(c) = &p.crash {
            keys.push((
                chip("Restart / try now"),
                if c.gave_up { "restart" } else { "try now" },
            ));
            if !c.gave_up {
                keys.push((chip("Sleep / stop trying"), "stop trying"));
            }
        } else if p.serving() {
            keys.push((chip("Sleep / stop trying"), "sleep"));
            keys.push((chip("Restart / try now"), "restart"));
            keys.push((chip("Pin awake"), if p.pinned { "unpin" } else { "pin" }));
        } else if !(p.on || p.paused) {
            keys.push((chip("Wake"), "wake"));
        }
    }
    keys.push((chip("Next tab"), "tabs"));
    keys_bar(
        t,
        keys,
        if v.tab == TAB_LOGS {
            ""
        } else {
            "from Overview: a card's name, or ⏎"
        },
    )
}

/// A keys bar of `(chip, what)` and a note at the right.
pub(crate) fn keys_bar(t: &Tokens, keys: Vec<(String, &str)>, note: &str) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .min_w_0()
        .items_center()
        .gap(px(16.))
        .h(px(34.))
        .px(px(28.))
        .border_t_1()
        .border_color(t.line)
        .text_size(px(ty::SM))
        .text_color(t.dim)
        .whitespace_nowrap()
        .overflow_hidden()
        .children(
            keys.into_iter()
                .filter(|(k, _)| !k.is_empty())
                .map(|(k, what)| {
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(5.))
                        .child(kbd_chip(t, &k, ty::XS, ty::SEMIBOLD, 6.))
                        .child(SharedString::from(what.to_owned()))
                }),
        )
        .child(div().flex_1())
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .child(ui::text(note)),
        )
}

/// A task's verb: what pressing it does now.
fn run_label(look: Option<Look>, ran: bool) -> &'static str {
    if look == Some(Look::Running) {
        "Running…"
    } else if ran {
        "Run again"
    } else {
        "Run"
    }
}
