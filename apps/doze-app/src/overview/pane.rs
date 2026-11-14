//! The List view's pane (`.insp`): the selected service in full (its addresses, how it runs, what it needs and what
//! holds it, its last error), or, for a stopped stack's row, what doze.hcl says of it.

use doze_desk::overview::{Pane, Row};
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use super::kv::Kv;

use super::tiles::{row_dot, word_tone};
use super::{OverviewView, all_rows};
use crate::kit::{Btn, Click, btn, chip_of, s, tone, word_line};
use crate::theme::{Tokens, ty};
use crate::widgets::{Mono as _, dot, tracked};

const PANE_W: f32 = 372.;

fn sec(t: &Tokens, title: &str) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(px(5.))
        .px(px(16.))
        .py(px(10.))
        .border_b_1()
        .border_color(t.line)
        .child(
            div()
                .mb(px(3.))
                .child(tracked(title, ty::XS, ty::SEMIBOLD, t.dim, 0.66)),
        )
}

fn frame(t: &Tokens) -> gpui::Stateful<gpui::Div> {
    div()
        .id("pane")
        .w(px(PANE_W))
        .flex_none()
        .flex()
        .flex_col()
        .min_h_0()
        .border_l_1()
        .border_color(t.line)
        .bg(t.side)
}

/// The pane's sections, which scroll; the actions stay under them, in view at any height.
fn sections() -> gpui::Stateful<gpui::Div> {
    div()
        .id("pane-sections")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
}

/// The pane for the selection.
pub(super) fn pane(t: &Tokens, v: &OverviewView, cx: &mut Context<OverviewView>) -> AnyElement {
    let row = v
        .selected
        .as_ref()
        .and_then(|n| all_rows(&v.home).into_iter().find(|r| &r.name == n))
        .cloned();
    match (row, &v.pane) {
        (Some(r), _) if r.down => down(t, &r, cx),
        (Some(r), Some(p)) if p.name == r.name => live(t, &r, p, cx),
        _ => frame(t).into_any_element(),
    }
}

/// `.ih.stack`: the dot and name, what runs it, its word and why.
fn head(t: &Tokens, r: &Row, p: &Pane) -> impl IntoElement {
    let sub = if !p.run.is_empty() && r.engine != p.run {
        format!("{} · {}", r.engine, p.run)
    } else {
        r.engine.clone()
    };
    let name = div()
        .text_size(px(ty::MD_UP))
        .font_weight(ty::HEADING)
        .text_color(t.bright)
        .child(s(&p.name));
    div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(px(4.))
        .px(px(16.))
        .pt(px(14.))
        .pb(px(10.))
        .border_b_1()
        .border_color(t.line)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(row_dot(t, r))
                .child(name),
        )
        .child(
            div()
                .text_size(px(ty::SM))
                .text_color(t.dim)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(s(&sub)),
        )
        .child(
            div()
                .text_size(px(ty::SM))
                .text_color(t.dim)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(word_line(p.look.word, word_tone(t, r), &[&p.because])),
        )
}

/// Run: the command, how long awake, the pid, when it sleeps.
fn run(t: &Tokens, r: &Row, p: &Pane, cx: &mut Context<OverviewView>) -> impl IntoElement {
    let mut run = sec(t, "Run");
    if !p.run.is_empty() {
        run = run.child(Kv::new("command", &p.run).copy(&p.run).el(t, cx));
    }
    if !p.up.is_empty() {
        run = run.child(Kv::new("awake for", &p.up).el(t, cx));
    }
    if p.pid > 0 {
        run = run.child(Kv::new("pid", &p.pid.to_string()).el(t, cx));
    }
    if r.kind != "task" {
        let sleeps = if r.pinned {
            "never: pinned awake".to_owned()
        } else {
            p.sleeps.clone()
        };
        run = run.child(
            Kv {
                wrap: true,
                verb: Some(if r.pinned { "Unpin" } else { "Pin" }),
                ..Kv::new("sleeps", &sleeps)
            }
            .el(t, cx),
        );
    }
    run
}

/// Needs · held by: a chip for each.
fn needs(t: &Tokens, p: &Pane) -> impl IntoElement {
    let chip = |glyph: AnyElement, text: &str| {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(22.))
            .px(px(8.))
            .rounded(px(6.))
            .bg(t.panel)
            .border_1()
            .border_color(t.line)
            .text_size(px(ty::SM))
            .child(glyph)
            .child(s(text))
    };
    let needs = p.needs.iter().map(|n| {
        chip(
            div()
                .text_color(tone(t, n.look.tone))
                .child(n.glyph)
                .into_any_element(),
            &n.name,
        )
    });
    let holders = p
        .holders
        .iter()
        .map(|h| chip(dot(t.good).into_any_element(), &h.text));
    sec(t, "Needs · held by")
        .child(div().flex().flex_wrap().gap(px(6.)).children(needs))
        .child(div().flex().flex_wrap().gap(px(6.)).children(holders))
}

fn open(cx: &mut Context<OverviewView>, name: &str, tab: Option<&'static str>) -> Click {
    let name = name.to_owned();
    Box::new(cx.listener(move |this, _, _, cx| this.open(&name, tab, cx)))
}

/// Logs, Restart; Open.
fn acts(t: &Tokens, r: &Row, cx: &mut Context<OverviewView>) -> impl IntoElement {
    div()
        .mt_auto()
        .flex()
        .flex_none()
        .gap(px(8.))
        .px(px(16.))
        .py(px(12.))
        .border_t_1()
        .border_color(t.line)
        .child(btn(
            t,
            "pane-logs",
            "Logs",
            "l",
            Btn::Plain,
            Some(open(cx, &r.name, Some(doze_display::TAB_LOGS))),
        ))
        .when(r.kind != "task", |d| {
            d.child(btn(
                t,
                "pane-restart",
                "Restart",
                "r",
                Btn::Plain,
                Some(Box::new(cx.listener(|this, _, _, cx| {
                    this.selected_verb(doze_control::Verb::Restart, cx)
                }))),
            ))
        })
        .child(div().flex_1())
        .child(btn(
            t,
            "pane-open",
            "Open",
            &chip_of("Open the service"),
            Btn::Primary,
            Some(open(cx, &r.name, None)),
        ))
}

fn live(t: &Tokens, r: &Row, p: &Pane, cx: &mut Context<OverviewView>) -> AnyElement {
    let mut f = sections().child(head(t, r, p));
    if !p.connect.is_empty() {
        let mut a = sec(t, "Addresses");
        for c in &p.connect {
            // `.kv .v.b` only on words: a mono value (an address) is the text's colour (overview.tsx).
            let bright = !c.dim && !c.mono;
            let kv = if c.copy.is_empty() {
                Kv {
                    bright,
                    ..Kv::new(&c.k, &c.v)
                }
            } else {
                let kv = Kv::new(&c.k, &c.v).copy(&c.copy);
                Kv { bright, ..kv }
            };
            let kv = Kv {
                note: &c.note,
                ..kv
            };
            a = a.child(kv.el(t, cx));
        }
        f = f.child(a);
    }
    f = f.child(run(t, r, p, cx));
    if !p.needs.is_empty() || !p.holders.is_empty() {
        f = f.child(needs(t, p));
    }
    if let Some(c) = p
        .crash
        .as_ref()
        .filter(|c| !c.error.is_empty() || !c.tries.is_empty())
    {
        let error = div()
            .px(px(10.))
            .py(px(8.))
            .rounded(px(7.))
            .bg(t.panel)
            .border_1()
            .border_color(t.line)
            .mono_face()
            .text_size(px(ty::SM))
            .text_color(t.bad)
            .child(s(&c.error));
        let next = if c.next_in.is_empty() {
            String::new()
        } else {
            format!(" · next in {}", c.next_in)
        };
        f = f.child(
            sec(t, "Last error")
                .child(error)
                .when(!c.tries.is_empty(), |d| {
                    d.child(
                        div()
                            .text_size(px(ty::SM))
                            .text_color(t.dim)
                            .child(s(&format!("{}{next}", c.tries))),
                    )
                }),
        );
    }
    frame(t).child(f).child(acts(t, r, cx)).into_any_element()
}

/// What Up does with a down row.
fn on_up(r: &Row) -> &'static str {
    if r.paused {
        "paused in doze.hcl"
    } else if r.kind == "task" {
        "runs when you say, once the stack is up"
    } else if r.last {
        "starts, as last time"
    } else if r.kind == "module" {
        "wakes when something connects"
    } else {
        "stays down; its switch starts it, with what it needs"
    }
}

/// `DownPane`: what doze.hcl says about a row of a stopped stack.
fn down(t: &Tokens, r: &Row, cx: &mut Context<OverviewView>) -> AnyElement {
    let head = div()
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(16.))
        .pt(px(14.))
        .pb(px(10.))
        .border_b_1()
        .border_color(t.line)
        .child(row_dot(t, r))
        .child(
            div()
                .text_size(px(ty::MD_UP))
                .font_weight(ty::HEADING)
                .text_color(t.bright)
                .child(s(&r.name)),
        )
        .child(
            div()
                .text_size(px(ty::SM))
                .text_color(t.dim)
                .child(s(&format!("{} · down", r.engine))),
        );
    let mut sec = sec(t, "From doze.hcl")
        .child(
            Kv::new(
                if r.kind == "module" {
                    "used by"
                } else {
                    "needs"
                },
                &r.because,
            )
            .el(t, cx),
        )
        .child(Kv::new("on Up", on_up(r)).el(t, cx));
    if !r.address.is_empty() {
        sec = sec.child(Kv::new("address", &r.address).copy(&r.address).el(t, cx));
    }
    if !r.run.is_empty() {
        sec = sec.child(Kv::new("command", &r.run).copy(&r.run).el(t, cx));
    }
    frame(t).child(head).child(sec).into_any_element()
}
