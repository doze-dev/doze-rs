//! The banners over the strip (Go `overview.tsx`, `app.tsx`): the daemon gone quiet at the top of the page; then a
//! running stack's doze.hcl that does not load (red, with the mistake shown where it is and Edit), doze.hcl changed
//! after the stack read it (with Restart the stack), and the stack's own domains not set up here (with Copy doze trust).
//! Each settles in as Go's `.banner` does (240 ms, from 6 px above).

use doze_desk::act::StackAction;
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use super::OverviewView;
use crate::actions::EditConfig;
use crate::kit::{Btn, btn, s};
use crate::outward::Probe as _;
use crate::theme::{Tokens, mix, ty};
use crate::widgets::{MONO_FAMILY, dot};

/// The mark at a banner's left: Go's `.dot` in the tone, or `.ring` (cool, 2 px) for the daemon's.
#[derive(Clone, Copy)]
enum Mark {
    Dot(gpui::Hsla),
    Ring,
}

/// `.banner`: the mark, a bold line and a dim one, tinted by its tone; buttons at the right.
fn banner(
    t: &Tokens,
    id: &'static str,
    (tone, mark): (gpui::Hsla, Mark),
    (line, sub): (&str, &str),
    buttons: Vec<AnyElement>,
) -> AnyElement {
    let mark = match mark {
        Mark::Dot(c) => dot(c).into_any_element(),
        Mark::Ring => div()
            .size(px(8.))
            .flex_none()
            .rounded_full()
            .border_2()
            .border_color(t.cool)
            .into_any_element(),
    };
    let el = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .mx(px(28.))
        .mb(px(12.))
        .px(px(14.))
        .py(px(10.))
        .rounded(px(10.))
        .border_1()
        .border_color(mix(tone, t.panel, 50.))
        .bg(mix(tone, t.panel, 10.))
        .child(mark)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(1.))
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .font_weight(ty::SEMIBOLD)
                        .text_color(t.bright)
                        .child(s(line)),
                )
                .when(!sub.is_empty(), |d| {
                    d.child(div().text_size(px(ty::SM)).text_color(t.dim).child(s(sub)))
                }),
        )
        .children(buttons)
        .probe(id, line.to_owned());
    crate::motion::settle(id, el, 240)
}

/// The daemon is not answering, or has gone quiet (Go: `app.tsx`'s warn banner over every stack page, a ring and
/// the words alone).
pub(super) fn stale(t: &Tokens, v: &OverviewView) -> Option<AnyElement> {
    let b = &v.home.banner;
    (!b.ok && !b.text.is_empty()).then(|| {
        banner(
            t,
            "banner-stale",
            (t.warn, Mark::Ring),
            (&b.text, ""),
            Vec::new(),
        )
    })
}

/// The banners of a stack's doze.hcl and domains, in Go's order.
pub(super) fn banners(
    t: &Tokens,
    v: &OverviewView,
    cx: &mut Context<OverviewView>,
) -> Vec<AnyElement> {
    let mut out = Vec::new();
    if let Some(c) = &v.home.config {
        let name = std::path::Path::new(&c.file).file_name().map_or_else(
            || "doze.hcl".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        let edit = btn(
            t,
            "edit-hcl-running",
            &format!("Edit {name}"),
            "⌘E",
            Btn::Plain,
            Some(Box::new(|_, window, cx| {
                window.dispatch_action(Box::new(EditConfig), cx);
            })),
        )
        .probe("edit-hcl-running", format!("Edit {name}"));
        out.push(banner(
            t,
            "banner-config",
            (t.bad, Mark::Dot(t.bad)),
            (
                &format!("{}: the stack keeps running as it was.", c.text),
                "Fix it in your editor; this page checks the file again each time it is saved. Once it loads, Restart the stack applies it: a running stack keeps the file it started with.",
            ),
            vec![edit.into_any_element()],
        ));
        out.push(crate::motion::settle(
            "srccard-running",
            super::strip::source_card(t, c),
            240,
        ));
    }
    if let Some(c) = &v.home.changed {
        let restart = btn(
            t,
            "restart-stack",
            "Restart the stack",
            "",
            Btn::Primary,
            Some(Box::new(cx.listener(|this, _, _, cx| {
                this.stack_act(StackAction::Restart, cx);
            }))),
        )
        .probe("restart-stack", "Restart the stack");
        out.push(banner(
            t,
            "banner-changed",
            (t.cool, Mark::Dot(t.cool)),
            (
                &format!("{}.", c.text),
                "The stack runs the file as it was when it came up. Restarting it brings it down and up as last time, on the new file; data stays.",
            ),
            vec![restart.into_any_element()],
        ));
    }
    if let Some(su) = &v.home.setup {
        out.push(setup(t, su, cx));
    }
    out
}

/// The domains' setup (Go `home.setup`): what is not reachable yet and what is missing. The machine's setup takes an
/// admin prompt, which this app does not raise: the command is offered to copy, for a terminal.
fn setup(
    t: &Tokens,
    su: &doze_desk::machine_names::Setup,
    cx: &mut Context<OverviewView>,
) -> AnyElement {
    let command = su.command.clone();
    let copy = div()
        .id("copy-trust")
        .flex()
        .flex_none()
        .items_center()
        .gap(px(5.))
        .h(px(30.))
        .px(px(12.))
        .rounded(px(7.))
        .text_size(px(ty::SM))
        .font_weight(ty::MEDIUM)
        .text_color(t.dim)
        .cursor_pointer()
        .hover(|d| d.text_color(t.bright))
        .child("Copy")
        .child(div().font_family(MONO_FAMILY).child(s(&su.command)))
        .probe("copy-trust", format!("Copy {}", su.command))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.copy_text(&command, "the command", cx);
        }));
    banner(
        t,
        "banner-setup",
        (t.cool, Mark::Dot(t.cool)),
        (&su.text, &su.detail),
        vec![copy.into_any_element()],
    )
}
