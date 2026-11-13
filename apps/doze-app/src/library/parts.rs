//! The Library's head, its help strip, the empty library, other programs' services and the machine line.

use doze_desk::library::{Check, Other, Step};
use gpui::{Context, IntoElement, div, img, prelude::*, px};

use super::LibraryView;
use crate::kit::{Btn, btn, chip_of, density, heading, mono, s};
use crate::theme::{Tokens, mix, ty};
use crate::widgets::{MONO_FAMILY, dot};

/// `.head.brandhead`: the mark, `doze` large, the tagline and the counts; density, Open folder, Settings.
pub(super) fn head(t: &Tokens, v: &LibraryView, cx: &mut Context<LibraryView>) -> impl IntoElement {
    let tagline = div()
        .flex()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_color(t.dim)
        .child("starts a service when something connects, and puts it to sleep when nothing does")
        .child(
            div()
                .text_color(t.text)
                .child(s(&format!(" · {}", v.lib.counts))),
        );
    let brand = div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .min_w_0()
        .child(
            div()
                .text_size(px(ty::BRAND))
                .line_height(px(ty::LH_44))
                .font_weight(ty::BRAND_WEIGHT)
                .text_color(t.bright)
                .child("doze"),
        )
        .child(tagline);
    let mut row = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(16.))
        .px(px(28.))
        .pt(px(28.))
        .pb(px(26.))
        .child(img("brand/doze.svg").size(px(64.)).flex_none())
        .child(brand)
        .child(div().flex_1());
    if !v.lib.stacks.is_empty() {
        let pick = |d| -> crate::kit::Click {
            let e = cx.entity();
            Box::new(move |_, _, cx| e.update(cx, |this, cx| this.set_density(d, cx)))
        };
        row = row.child(density(t, v.density, pick));
    }
    row.child(btn(
        t,
        "open-folder",
        "Open folder…",
        &chip_of("Open folder…"),
        Btn::Plain,
        Some(dispatch(&crate::actions::OpenFolder)),
    ))
    .child(btn(
        t,
        "settings",
        "Settings",
        &chip_of("Settings…"),
        Btn::Quiet,
        Some(say(cx, SOON_SETTINGS)),
    ))
}

/// The few lines the empty library shows (library.tsx).
const EXAMPLE: &str = "stack = \"shop\"\n\napp \"web\" {\n  run  = \"pnpm dev\"\n  port = 3000\n}\n\nmodule postgres \"db\" { version = 17 }";
/// A click that says `words` at the foot.
fn say(cx: &mut Context<LibraryView>, words: &'static str) -> crate::kit::Click {
    Box::new(cx.listener(move |this, _, _, cx| this.say(words, cx)))
}

/// A click that does what the action table's row does (Open folder ⌘O, New stack ⌘N).
fn dispatch(a: &dyn gpui::Action) -> crate::kit::Click {
    let a = a.boxed_clone();
    Box::new(move |_, window, cx| window.dispatch_action(a.boxed_clone(), cx))
}
const SOON_SETTINGS: &str =
    "Settings come with M3; the theme cycles with ⌘⇧T and the mode with ⌘⇧L.";

/// One step: its number (ticked when done, ringed when it is the one to do now), its title and detail.
fn step(t: &Tokens, i: usize, last: bool, st: &Step) -> impl IntoElement {
    let (bg, border, fg) = if st.done {
        (t.good, t.good, t.ink)
    } else if st.now {
        (gpui::transparent_black(), t.accent, t.accent)
    } else {
        (gpui::transparent_black(), t.line2, t.dim)
    };
    let num = div()
        .size(px(24.))
        .flex_none()
        .rounded_full()
        .border_1()
        .border_color(border)
        .bg(bg)
        .flex()
        .items_center()
        .justify_center()
        .font_family(MONO_FAMILY)
        .text_size(px(ty::SM))
        .font_weight(ty::SEMIBOLD)
        .text_color(fg)
        .child(if st.done {
            s("✓")
        } else {
            s(&(i + 1).to_string())
        });
    let line = |d: gpui::Div| d.whitespace_nowrap().overflow_hidden().text_ellipsis();
    let title =
        line(
            div()
                .font_weight(ty::SEMIBOLD)
                .text_color(if st.done { t.dim } else { t.bright }),
        )
        .child(st.title);
    let detail = line(div().text_size(px(ty::SM)).text_color(t.dim)).child(s(&st.detail));
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap(px(12.))
        .px(px(16.))
        .py(px(12.))
        .when(!last, |d| d.border_r_1().border_color(t.line))
        .child(num)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(1.))
                .min_w_0()
                .child(title)
                .child(detail),
        )
}

/// `.steps`: the three steps; "Got it ×".
pub(super) fn steps(t: &Tokens, steps: &[Step], cx: &mut Context<LibraryView>) -> impl IntoElement {
    let n = steps.len();
    let got_it = div()
        .id("got-it")
        .flex()
        .flex_none()
        .items_center()
        .px(px(14.))
        .border_l_1()
        .border_color(t.line)
        .text_size(px(ty::SM))
        .text_color(t.dim)
        .cursor_pointer()
        .hover(|d| d.text_color(t.bright))
        .child("Got it ×")
        .on_click(cx.listener(|this, _, _, cx| this.dismiss_steps(cx)));
    div()
        .flex()
        .flex_none()
        .mx(px(28.))
        .mb(px(14.))
        .rounded(px(10.))
        .border_1()
        .border_color(t.line)
        .bg(t.panel)
        .overflow_hidden()
        .children(
            steps
                .iter()
                .enumerate()
                .map(|(i, st)| step(t, i, i + 1 == n, st)),
        )
        .child(got_it)
}

/// The LibraryEmpty board: two ways in, and the one idea.
pub(super) fn empty(t: &Tokens, _cx: &mut Context<LibraryView>) -> impl IntoElement {
    let way = || {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(px(8.))
            .p(px(20.))
            .rounded(px(10.))
            .border_1()
            .bg(t.panel)
    };
    let h3 = |text: &'static str| {
        div()
            .text_size(px(ty::MD_UP))
            .font_weight(ty::HEADING)
            .text_color(t.bright)
            .child(text)
    };
    let p = |text: &'static str| {
        div()
            .text_size(px(ty::SM))
            .line_height(px(ty::LH_18))
            .text_color(t.dim)
            .child(text)
    };
    let drop = way()
        .border_dashed()
        .border_color(t.line2)
        .items_center()
        .text_center()
        .child(h3("Drop a project folder here"))
        .child(p("doze reads what is in it: go.mod, package.json, a compose file, a Makefile. It says what it found before anything starts."))
        .child(div().flex_1().min_h(px(150.)))
        .child(btn(t, "drop-open", "Open folder…", &chip_of("Open folder…"), Btn::Primary, Some(dispatch(&crate::actions::OpenFolder))));
    let example = div()
        .my(px(4.))
        .px(px(10.))
        .py(px(8.))
        .rounded(px(7.))
        .border_1()
        .border_color(t.line)
        .bg(t.well)
        .children(EXAMPLE.lines().map(|l| {
            mono(s(if l.is_empty() { " " } else { l }), ty::XS_UP, t.dim).line_height(px(ty::LH_18))
        }));
    let start = way()
        .border_color(t.line)
        .child(h3("Or start empty"))
        .child(p("A new folder with a doze.hcl of a few lines. Add one thing at a time: postgres, valkey, kafka, an app of your own."))
        .child(example)
        .child(div().flex().child(btn(t, "new-stack", "New stack…", &chip_of("New stack…"), Btn::Plain, Some(dispatch(&crate::actions::NewStack)))));
    div()
        .flex()
        .flex_col()
        .flex_none()
        .child(div().flex().gap(px(14.)).mx(px(28.)).mb(px(14.)).child(drop).child(start))
        .child(
            div()
                .mx(px(28.))
                .text_size(px(ty::SM_UP))
                .line_height(px(ty::LH_18))
                .text_color(t.dim)
                .child("The one idea behind everything here: doze starts a service when something connects to it, and puts it to sleep when nothing does. A row saying dozing is counting down; asleep wakes on the next connection; pinned never sleeps. Data stays through all of it."),
        )
}

/// `.model`: the line under the table.
pub(super) fn model(t: &Tokens) -> impl IntoElement {
    div()
        .mx(px(28.))
        .mt(px(12.))
        .text_size(px(ty::SM_UP))
        .line_height(px(ty::LH_18))
        .text_color(t.dim)
        .child("Drop a project folder anywhere in this window, or press ⌘O, to add it. It appears here, stopped, until you bring it up, and stays until you remove it (⌫), which stops it and leaves its files alone.")
}

/// `.others`: what other doze programs run here.
pub(super) fn others(t: &Tokens, others: &[Other]) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(px(4.))
        .mx(px(28.))
        .mt(px(14.))
        .px(px(14.))
        .py(px(10.))
        .rounded(px(10.))
        .border_1()
        .border_color(t.line)
        .bg(t.panel)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .mb(px(4.))
                .child(heading("Also on this machine", t.dim, ty::SEMIBOLD, 0.66))
                .child(
                    div()
                        .text_size(px(ty::XS))
                        .text_color(t.dim)
                        .child("· run by other doze programs, not by a stack"),
                ),
        )
        .children(others.iter().enumerate().map(|(i, o)| {
            let url = o.url.clone();
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .min_h(px(30.))
                .child(mono(s(&o.name), ty::SM, t.bright))
                .child(
                    div()
                        .text_size(px(ty::SM))
                        .text_color(t.dim)
                        .child(s(&format!("{} · pid {}", o.owner, o.pid))),
                )
                .child(div().flex_1())
                .when(!url.is_empty(), |d| {
                    d.child(btn(
                        t,
                        ("other", i),
                        "Open",
                        "",
                        Btn::Quiet,
                        Some(Box::new(move |_, _, cx| cx.open_url(&url))),
                    ))
                })
        }))
}

/// `.machine`: each part of the setup and whether it is in place; Machine… opens Settings (M3).
pub(super) fn machine_line(
    t: &Tokens,
    checks: &[Check],
    cx: &mut Context<LibraryView>,
) -> impl IntoElement {
    let mut line = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(14.))
        .h(px(40.))
        .mx(px(28.))
        .mt(px(12.))
        .mb(px(12.))
        .px(px(14.))
        .rounded(px(10.))
        .border_1()
        .border_color(t.line)
        .bg(t.well)
        .text_size(px(ty::SM))
        .text_color(t.dim)
        .whitespace_nowrap()
        .overflow_hidden()
        .child(
            div()
                .flex_none()
                .font_weight(ty::SEMIBOLD)
                .text_color(t.text)
                .child("Machine"),
        );
    if checks.is_empty() {
        line = line.child("checking…");
    }
    for c in checks {
        let mut it = div()
            .flex()
            .flex_shrink(1.)
            .min_w_0()
            .items_center()
            .gap(px(5.))
            .overflow_hidden()
            .child(dot(if c.ok { t.good } else { t.warn }))
            .child(
                div()
                    .flex_none()
                    .text_color(mix(t.text, t.dim, 50.))
                    .child(s(&c.name)),
            );
        if !c.ok && !c.text.is_empty() {
            it = it.child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(s(&format!(" · {}", c.text))),
            );
        }
        line = line.child(it);
    }
    line.child(div().flex_1()).child(
        btn(
            t,
            "machine",
            "Machine…",
            "",
            Btn::Plain,
            Some(Box::new(
                cx.listener(|this, _, _, cx| this.say(SOON_SETTINGS, cx)),
            )),
        )
        .h(px(26.)),
    )
}
