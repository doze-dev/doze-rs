//! The overview tab, as the Instance, Cache and Crash boards lay it out: a crashing service's banner, the port clash,
//! what it said; then the connect card (with the snippets for a module), why it is in its state, what happened this
//! run; and on the right the charts. A module's own blocks go after the card it names.

use doze_desk::page::instance::{InstancePage, need_tone};
use doze_desk::page::parts::Countdown;
use doze_display::Look;
use gpui::{AnyElement, Context, IntoElement, Window, div, prelude::*, px, relative};
use vitals::charts;

use super::InstanceView;
pub(crate) use super::asleep::{asleep, web_console};
use super::clash;
use super::snippets;
use super::ui::{self, banner, card, heading, kv_row, pill, text};
use crate::console::blocks;
use crate::theme::{Tokens, ty};
use crate::widgets::MONO_FAMILY;

mod vitals;

pub(crate) fn overview(
    t: &Tokens,
    v: &mut InstanceView,
    _window: &mut Window,
    cx: &mut Context<InstanceView>,
) -> AnyElement {
    let p = v.page.clone();
    let module = p.kind == doze_display::Kind::Module;
    let mut left = div().flex().flex_col().gap(px(14.)).flex_1().min_w_0();
    if let Some(c) = &p.crash
        && !c.last_run.is_empty()
    {
        left = left.child(said(t, v, cx));
    }
    if !p.connect.is_empty() {
        left = left.child(connect(
            t,
            v,
            if module { "Connect" } else { "Reach it" },
            None,
            cx,
        ));
    }
    if module {
        left = left.children(snippets::snippets(t, v, cx));
    }
    left = left.children(extends(t, &p, "connect"));
    if p.crash.is_none() {
        left = left.child(state(t, &p, cx));
    }
    left = left.children(extends(t, &p, "awake"));
    left = left
        .child(recent(t, &p).flex_1())
        .children(extends(t, &p, "recent"));
    let right = div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .flex_1()
        .min_w_0()
        .children(charts(t, &p));
    let top = crash_top(t, v, &p, cx);
    div()
        .id("overview")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(top)
        .child(
            div()
                .flex()
                .flex_1()
                .gap(px(14.))
                .px(px(28.))
                .py(px(16.))
                .child(left.flex_grow(1.2).flex_basis(relative(0.)))
                .child(right.flex_grow(1.).flex_basis(relative(0.))),
        )
        .into_any_element()
}

/// Above the cards: a crashing service's banner, and the port clash.
/// The crash banner's two lines. Not ready in time is its own failure (Needs you says so: "did not become ready"), and
/// a page with nothing to show from the last run points at the logs instead; a failed one is not tried again, so there
/// is nothing to stop.
fn crash_words(c: &doze_desk::page::parts::Crash) -> (String, &'static str) {
    let not_ready = c.error.contains("not become ready");
    let sub = match (c.last_run.is_empty(), c.gave_up || not_ready) {
        (true, true) => "Its logs usually say why. Fix it and press Restart.",
        (true, false) => {
            "Its logs usually say why. Fix it and press Try now, or stop trying while you look."
        }
        (false, true) => "What it said is below. Fix it and press Restart.",
        (false, false) => {
            "What it said is below. Fix it and press Try now, or stop trying while you look."
        }
    };
    let said = if not_ready {
        "It started but never answered, so doze stopped it. It waits for you.".to_owned()
    } else if c.gave_up {
        "It kept exiting soon after starting, so doze stopped trying. It waits for you.".to_owned()
    } else {
        let next = if c.next_in.is_empty() {
            String::new()
        } else {
            format!(" in {}", c.next_in)
        };
        let tries = if c.tries.is_empty() {
            String::new()
        } else {
            format!(" ({})", c.tries)
        };
        format!(
            "It exits soon after starting. doze tries again{next}{tries}, and stops trying if it keeps exiting."
        )
    };
    (said, sub)
}

fn crash_top(
    t: &Tokens,
    v: &mut InstanceView,
    p: &InstancePage,
    cx: &mut Context<InstanceView>,
) -> gpui::Div {
    let mut top = div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .px(px(28.))
        .when(p.crash.is_some() || !p.stuck.is_empty(), |d| d.pt(px(14.)))
        .flex_none();
    if p.crash.is_none() && !p.stuck.is_empty() {
        // Stuck waking: Needs you has it as a red item; the page says the same, with the same verb (Restart).
        // Needs you's words, once: "for 7m, with no word from it for 7m" says the time twice on one line.
        let long = p
            .stuck
            .split(',')
            .next()
            .unwrap_or("")
            .trim_start_matches("for ");
        top = top.child(banner(
            t,
            ui::Tone::Warn,
            &format!("It has been waking for {long} and has said nothing."),
            "Its logs usually say why. Restart starts it again.",
        ));
    }
    if let Some(c) = &p.crash {
        let (said, sub) = crash_words(c);
        let mut b = banner(t, ui::Tone::Bad, &said, sub);
        if !c.tries.is_empty() {
            b = b.child(
                div()
                    .flex_none()
                    .h(px(22.))
                    .px(px(9.))
                    .flex()
                    .items_center()
                    .rounded(px(11.))
                    .bg(t.panel)
                    .border_1()
                    .border_color(crate::theme::mix(t.bad, t.panel, 55.))
                    .text_size(px(ty::SM))
                    .text_color(t.bad)
                    .child(text(&c.tries)),
            );
        }
        top = top.child(b);
        if c.port.is_some() {
            top = top.children(clash::clash(t, v, cx));
        }
    }
    top
}

/// "What it said": the last lines before it exited, and the error.
fn said(t: &Tokens, v: &InstanceView, cx: &mut Context<InstanceView>) -> AnyElement {
    let Some(c) = &v.page.crash else {
        return div().into_any_element();
    };
    let all: Vec<String> = c
        .last_run
        .iter()
        .map(|l| format!("{} {}", l.time, l.text))
        .chain(
            [c.error.clone(), c.r#where.clone()]
                .into_iter()
                .filter(|s| !s.is_empty()),
        )
        .collect();
    let joined = all.join("\n");
    let copied = v.is_copied("said");
    let mut code = ui::code(t);
    for l in &c.last_run {
        code = code.child(
            div()
                .flex()
                .gap(px(8.))
                .child(div().flex_none().text_color(t.dim).child(text(&l.time)))
                .child(
                    div()
                        .min_w_0()
                        .text_color(ui::ink(t, l.ink()))
                        .child(text(&l.text)),
                ),
        );
    }
    if !c.error.is_empty() {
        code = code.child(div().text_color(t.bad).child(text(&c.error)));
    }
    if !c.r#where.is_empty() {
        code = code.child(div().text_color(t.dim).child(text(&c.r#where)));
    }
    card(t)
        .child(
            heading(t, "What it said", Some("· the last lines before it exited"))
                .child(div().flex_1())
                .child(
                    ui::copy_link(t, "copy-said", if copied { "Copied" } else { "Copy" })
                        .on_click(cx.listener(move |v, _, _, cx| v.copy("said", &joined, cx))),
                ),
        )
        .child(code)
        .into_any_element()
}

/// The connect card: its rows, each with a copy link.
pub(crate) fn connect(
    t: &Tokens,
    v: &InstanceView,
    title: &str,
    sub: Option<&str>,
    cx: &mut Context<InstanceView>,
) -> AnyElement {
    let mut c = card(t).child(heading(t, title, sub));
    for r in &v.page.connect {
        let (id, copy) = (format!("kv-{}", r.k), r.copy.clone());
        let id2 = id.clone();
        c = c.child(kv_row(
            t,
            r,
            v.is_copied(&id),
            cx.listener(move |v, _, _, cx| v.copy(&id2, &copy, cx)),
        ));
    }
    c.into_any_element()
}

/// The state card: why it is awake (the holders), the dozing countdown, the sentence, what it needs.
fn state(t: &Tokens, p: &InstancePage, cx: &mut Context<InstanceView>) -> AnyElement {
    // The card's heading by the look itself, not its word: a word the look renames cannot leave a card unheaded.
    let title = match p.look {
        Some(Look::Awake) => "Why it's awake",
        Some(Look::Dozing) => "Dozing",
        Some(Look::Waking) => "Waking",
        Some(Look::Asleep) => "Asleep",
        Some(Look::Off) => "Turned off",
        Some(Look::Paused) => "Paused in doze.hcl",
        Some(Look::Failed) => "Failed",
        Some(l) => l.word(),
        None => "",
    };
    let mut c = card(t).child(heading(t, title, None));
    for h in &p.holders {
        c = c.child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(pill(t, t.good, text(&h.text)))
                .child(div().text_color(t.dim).child(text(&h.explain))),
        );
    }
    if let Some(d) = &p.dozing {
        c = c.child(countdown(t, d));
    }
    let quiet = p.holders.is_empty() && p.dozing.is_none();
    if !p.sleeps.is_empty() {
        c = c.child(
            div()
                .text_color(if quiet { t.bright } else { t.dim })
                .child(text(&p.sleeps)),
        );
    }
    if !p.needs.is_empty() {
        c = c.child(needs(t, p, cx));
    }
    c.into_any_element()
}

/// The dozing countdown: how long until it sleeps, and the bar running down.
fn countdown(t: &Tokens, d: &Countdown) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(pill(
            t,
            t.warn,
            div()
                .text_color(t.warn)
                .child(text(&format!("sleeps in {}", d.left))),
        ))
        .child(
            div()
                .flex_1()
                .h(px(8.))
                .rounded(px(4.))
                .bg(t.track)
                .overflow_hidden()
                .child(div().h_full().w(relative(d.fraction as f32)).bg(t.warn)),
        )
}

/// What it needs: a chip for each, which opens the one it names when it is known.
fn needs(t: &Tokens, p: &InstancePage, cx: &mut Context<InstanceView>) -> gpui::Div {
    let mut chips = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.))
        .mt(px(4.))
        .child(div().text_size(px(ty::SM)).text_color(t.dim).child("needs"));
    for (i, n) in p.needs.iter().enumerate() {
        let name = n.name.clone();
        let known = n.known;
        chips = chips.child(
            div()
                .id(("need", i))
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(22.))
                .px(px(8.))
                .rounded(px(6.))
                .border_1()
                .border_color(t.line)
                .bg(t.panel)
                .text_size(px(ty::SM))
                .when(known, |d| d.cursor_pointer().hover(|s| s.bg(t.sel)))
                .on_click(cx.listener(move |_, _, _, cx| {
                    if known {
                        cx.emit(super::InstanceEvent::Open(name.clone()));
                    }
                }))
                .child(div().text_color(t.tone(need_tone(n))).child(n.glyph))
                .child(text(&n.name)),
        );
    }
    chips
}

/// "Recent · this run": doze's events for it.
fn recent(t: &Tokens, p: &InstancePage) -> gpui::Div {
    let mut rows = div()
        .flex()
        .flex_col()
        .font_family(MONO_FAMILY)
        .text_size(px(ty::SM))
        .line_height(px(ty::LH_21));
    if p.recent.is_empty() {
        rows = rows.child(div().text_color(t.dim).child("nothing yet"));
    }
    for r in &p.recent {
        rows = rows.child(
            div()
                .flex()
                .gap(px(10.))
                .child(
                    div()
                        .w(px(64.))
                        .flex_none()
                        .text_color(t.dim)
                        .child(text(&r.at)),
                )
                .child(
                    div()
                        .w(px(96.))
                        .flex_none()
                        .text_color(ui::ink(t, r.ink))
                        .child(text(&r.kind)),
                )
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(text(&r.text)),
                ),
        );
    }
    card(t)
        .child(heading(t, "Recent", Some("· this run")))
        .child(rows)
}

/// The module's blocks for the overview that go after the card `after`.
fn extends(t: &Tokens, p: &InstancePage, after: &str) -> Vec<AnyElement> {
    p.extend
        .iter()
        .filter(|e| e.after == after)
        .map(|e| {
            let mut n = e.node.clone();
            let title = std::mem::take(&mut n.title);
            n.flexible = false;
            let mut c = card(t).px(px(0.)).py(px(0.)).min_w_0().overflow_hidden();
            if !title.is_empty() {
                c = c.child(
                    div()
                        .px(px(14.))
                        .pt(px(12.))
                        .child(heading(t, &title, None)),
                );
            }
            c.child(blocks::static_block(t, &n)).into_any_element()
        })
        .collect()
}
