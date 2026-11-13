//! Alerts and the away strip, over the page. An alert is a red item that appeared while the window was open (Go
//! `Alerts`): its first fix and Show, until acted on or closed; it stays in Needs you either way. The away strip is
//! what happened while the window was closed (the WhileAway board, Go `AwayStrip`), said once: Open Needs you or Got
//! it puts it away. Both float over the page, so nothing under them moves, and enter gently.

use doze_desk::attention::{FixKind, Item, away::Away};
use gpui::{
    Animation, AnimationExt as _, AnyElement, Context, IntoElement, SharedString, div, prelude::*,
    px,
};

use crate::kit::{Btn, btn};
use crate::shell::Root;
use crate::theme::{Motion, Tokens, alpha};
use crate::widgets::{MONO_FAMILY, w};

/// Fades and lifts `el` in, unless motion is reduced.
fn enter(el: gpui::Div, id: SharedString, motion: Motion) -> AnyElement {
    if !motion.moves() {
        return el.into_any_element();
    }
    let travel = motion.travel;
    el.with_animation(
        id,
        Animation::new(motion.normal).with_easing(gpui::ease_out_quint()),
        move |d, p| d.opacity(p).mt(px(travel * (1.0 - p))),
    )
    .into_any_element()
}

/// An alert's buttons: its first fix that is not Show, and Show unless its page is the one on screen.
fn alert_fixes(t: &Tokens, it: &Item, here: bool, cx: &mut Context<Root>) -> gpui::Div {
    let mut fx = div().flex().gap(px(6.)).mt(px(6.));
    if let Some(f) = it.fixes.iter().find(|f| f.kind != FixKind::Show).cloned() {
        let item = it.clone();
        fx = fx.child(
            btn(
                t,
                SharedString::from(format!("al-fix-{}", it.id)),
                &f.label,
                "",
                Btn::Primary,
                None,
            )
            .h(px(24.))
            .px(px(10.))
            .on_click(cx.listener(move |r, _, window, cx| r.fix_item(&item, &f, window, cx))),
        );
    }
    if it.show.is_some() && !here {
        let item = it.clone();
        fx = fx.child(
            btn(
                t,
                SharedString::from(format!("al-show-{}", it.id)),
                "Show",
                "",
                Btn::Plain,
                None,
            )
            .h(px(24.))
            .px(px(10.))
            .on_click(cx.listener(move |r, _, window, cx| r.show_item(&item, window, cx))),
        );
    }
    fx
}

fn alert(t: &Tokens, it: &Item, here: bool, cx: &mut Context<Root>) -> gpui::Div {
    let fx = alert_fixes(t, it, here, cx);
    let id = it.id.clone();
    div()
        .w(px(380.))
        .flex()
        .items_start()
        .gap(px(10.))
        .p(px(12.))
        .rounded(px(10.))
        .border_1()
        .border_color(alpha(t.bad, 0.6))
        .bg(t.panel)
        .shadow(vec![super::shadow(t)])
        .child(
            div()
                .mt(px(5.))
                .size(px(9.))
                .flex_none()
                .rounded_full()
                .bg(t.bad),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .font_weight(w(600.))
                        .text_color(t.bright)
                        .child(SharedString::from(if it.stack.is_empty() {
                            it.title.clone()
                        } else {
                            format!("{} · {}", it.title, it.stack)
                        })),
                )
                .when(!it.detail.is_empty(), |d| {
                    let line = div()
                        .text_size(px(12.))
                        .text_color(t.dim)
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(SharedString::from(it.detail.clone()));
                    d.child(if it.mono {
                        line.font_family(MONO_FAMILY).text_size(px(11.5))
                    } else {
                        line
                    })
                })
                .child(fx),
        )
        .child(
            div()
                .id(SharedString::from(format!("al-x-{}", it.id)))
                .flex_none()
                .px(px(4.))
                .text_color(t.dim)
                .cursor_pointer()
                .hover(|d| d.text_color(t.bright))
                .child("×")
                .on_click(cx.listener(move |r, _, _, cx| r.close_alert(&id, cx))),
        )
}

/// The alerts, bottom right, newest last.
pub(crate) fn alerts(
    t: &Tokens,
    items: &[Item],
    here: &dyn Fn(&Item) -> bool,
    cx: &mut Context<Root>,
) -> Option<AnyElement> {
    if items.is_empty() {
        return None;
    }
    let motion = Motion::of(cx);
    let stack = div()
        .flex()
        .flex_col()
        .items_end()
        .gap(px(8.))
        .children(items.iter().map(|it| {
            enter(
                alert(t, it, here(it), cx),
                SharedString::from(format!("al-in-{}", it.id)),
                motion,
            )
        }));
    Some(
        div()
            .absolute()
            .right(px(18.))
            .bottom(px(54.))
            .child(stack)
            .into_any_element(),
    )
}

/// The strip's first line: what it is, for how long, and its two buttons.
fn away_head(t: &Tokens, a: &Away, cx: &mut Context<Root>) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(div().size(px(8.)).rounded_full().bg(t.cool))
        .child(
            div()
                .font_weight(w(650.))
                .text_color(t.bright)
                .child("While the window was closed"),
        )
        .child(div().text_color(t.dim).child(SharedString::from(format!(
            "{} · the stacks ran on in the menu bar",
            a.r#for
        ))))
        .child(div().flex_1())
        .child(
            btn(t, "away-bell", "Open Needs you", "", Btn::Plain, None)
                .h(px(26.))
                .on_click(cx.listener(|r, _, window, cx| r.away_to_bell(window, cx))),
        )
        .child(
            btn(t, "away-ok", "Got it", "", Btn::Quiet, None)
                .h(px(26.))
                .on_click(cx.listener(|r, _, _, cx| r.away_done(cx))),
        )
}

/// One line of the strip: when, who, what, how it stands now; a click opens the service.
fn away_row(
    t: &Tokens,
    i: usize,
    e: &doze_desk::attention::away::AwayEvent,
    cx: &mut Context<Root>,
) -> gpui::Stateful<gpui::Div> {
    let tone = match e.tone {
        "bad" => t.bad,
        "good" => t.good,
        _ => t.dim,
    };
    let (key, inst) = (e.key.clone(), e.instance.clone());
    div()
        .id(("away", i))
        .border_t_1()
        .border_color(alpha(t.cool, 0.25))
        .px(px(14.))
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(34.))
        .cursor_pointer()
        .hover(|d| d.bg(t.sel))
        .on_click(cx.listener(move |r, _, window, cx| r.away_open(&key, &inst, window, cx)))
        .child(div().size(px(7.)).rounded_full().bg(tone))
        .child(
            div()
                .w(px(84.))
                .flex_none()
                .font_family(MONO_FAMILY)
                .text_size(px(11.5))
                .text_color(t.dim)
                .child(SharedString::from(e.at.clone())),
        )
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .gap(px(5.))
                .child(
                    div()
                        .font_weight(w(600.))
                        .text_color(t.bright)
                        .child(SharedString::from(e.instance.clone())),
                )
                .child(
                    div()
                        .text_color(t.text)
                        .child(SharedString::from(e.what.clone())),
                ),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(12.))
                .text_color(tone)
                .child(SharedString::from(e.now.clone())),
        )
}

/// The away strip, under the title bar.
pub(crate) fn away(t: &Tokens, a: &Away, cx: &mut Context<Root>) -> AnyElement {
    let head = away_head(t, a, cx);
    let rows = a
        .events
        .iter()
        .enumerate()
        .map(|(i, e)| away_row(t, i, e, cx));
    let strip = div()
        .flex()
        .flex_col()
        .rounded(px(12.))
        .border_1()
        .border_color(alpha(t.cool, 0.45))
        .bg(alpha(t.cool, 0.08))
        .text_size(px(13.))
        .child(head.px(px(14.)).py(px(10.)))
        .child(div().flex().flex_col().children(rows));
    // In the page, above it: what it covers would be the stack's own head, so it takes its place for the moment it is
    // read, entering gently, and the page closes up when it is put away.
    div()
        .flex_none()
        .px(px(28.))
        .pt(px(12.))
        .child(enter(div().child(strip), "away-in".into(), Motion::of(cx)))
        .into_any_element()
}
