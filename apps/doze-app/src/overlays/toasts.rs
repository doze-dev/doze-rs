//! What the app says (Go `components/toasts.tsx`): toasts at the bottom right, at most five; a success goes after 4 s,
//! a failure stays 12 s or until dismissed, with "All" for the Recent popover. Recent (Go's Recent page, made an
//! overlay: PLAN.md row 74) opens over the toast stack, anchored to it: the last twenty lines, newest first. Each toast
//! rises in (220 ms); none moves while the system reduces motion. It redraws only when what was said changes.

use std::time::SystemTime;

use doze_desk::act::{Notice, Toast};
use gpui::{
    Animation, AnimationExt as _, AnyElement, Context, Entity, IntoElement, Render, SharedString,
    Window, div, prelude::*, px,
};

use crate::act::Acts;
use crate::kit::s;
use crate::theme::{Motion, Tokens, tokens};
use crate::widgets::{MONO_FAMILY, UI_FAMILY, icon, kbd_plain, w};

/// The toast stack and the Recent popover.
pub(crate) struct ToastsView {
    acts: Entity<Acts>,
}

impl ToastsView {
    pub(crate) fn new(acts: Entity<Acts>, cx: &mut Context<Self>) -> ToastsView {
        cx.observe(&acts, |_, _, cx| cx.notify()).detach();
        ToastsView { acts }
    }
}

/// A clock time, as the Recent lines show it.
fn clock(at: SystemTime) -> String {
    let local: chrono::DateTime<chrono::Local> = at.into();
    local.format("%H:%M:%S").to_string()
}

fn toast(t: &Tokens, x: &Toast, motion: Motion, cx: &mut Context<ToastsView>) -> AnyElement {
    let (id, bad) = (x.id, x.notice.bad);
    let mut el =
        div()
            .id(("toast", id))
            .flex()
            .items_center()
            .gap(px(10.))
            .max_w(px(420.))
            .px(px(14.))
            .py(px(10.))
            .rounded(px(10.))
            .border_1()
            .border_color(if bad { t.bad } else { t.line2 })
            .bg(t.panel)
            .shadow(vec![super::shadow(t)])
            .child(div().size(px(10.)).flex_none().rounded_full().bg(if bad {
                t.bad
            } else {
                t.good
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_weight(w(600.))
                    .text_color(t.bright)
                    .line_clamp(3)
                    .child(s(&x.notice.text)),
            );
    if bad {
        let all = div()
            .id(("toast-all", id))
            .flex_none()
            .h(px(24.))
            .px(px(8.))
            .flex()
            .items_center()
            .rounded(px(6.))
            .text_size(px(12.))
            .text_color(t.dim)
            .cursor_pointer()
            .hover(|d| d.text_color(t.bright).bg(t.sel))
            .child("All")
            .on_click(
                cx.listener(|this, _, _, cx| this.acts.update(cx, |a, cx| a.toggle_recent(cx))),
            );
        let close = close_x(t, ("toast-x", id).into(), cx, move |this, cx| {
            this.acts.update(cx, |a, cx| a.dismiss(id, cx));
        });
        el = el.child(all).child(close);
    }
    if !motion.moves() {
        return el.into_any_element();
    }
    let travel = motion.travel * 1.5;
    el.with_animation(
        SharedString::from(format!("toast-in-{id}")),
        Animation::new(motion.normal + motion.normal / 4).with_easing(gpui::ease_out_quint()),
        move |d, p| d.opacity(p).mt(px(travel * (1.0 - p))),
    )
    .into_any_element()
}

fn line(t: &Tokens, n: &Notice) -> AnyElement {
    div()
        .flex()
        .items_start()
        .gap(px(10.))
        .px(px(14.))
        .py(px(6.))
        .border_t_1()
        .border_color(t.line)
        .text_size(px(12.))
        .child(
            div()
                .w(px(58.))
                .flex_none()
                .font_family(MONO_FAMILY)
                .text_size(px(11.5))
                .text_color(t.dim)
                .child(clock(n.at)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_color(if n.bad { t.bad } else { t.text })
                .child(if n.bad {
                    format!("✗ {}", n.text)
                } else {
                    n.text.clone()
                }),
        )
        .into_any_element()
}

/// The Recent popover.
fn recent(t: &Tokens, lines: &[Notice], cx: &mut Context<ToastsView>) -> AnyElement {
    let head = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(14.))
        .py(px(10.))
        .child(
            div()
                .font_weight(w(600.))
                .text_color(t.bright)
                .child("Recent messages"),
        )
        .child(
            div()
                .flex_1()
                .text_size(px(12.))
                .text_color(t.dim)
                .child("the last 20"),
        )
        .child(kbd_plain("esc", 11., t.dim, 500.))
        .child(close_x(t, "recent-x".into(), cx, |this, cx| {
            this.acts.update(cx, |a, cx| a.toggle_recent(cx));
        }));
    let body: Vec<AnyElement> = if lines.is_empty() {
        vec![
            div()
                .px(px(14.))
                .py(px(8.))
                .border_t_1()
                .border_color(t.line)
                .text_size(px(12.))
                .text_color(t.dim)
                .italic()
                .child("nothing has been said yet")
                .into_any_element(),
        ]
    } else {
        lines.iter().map(|n| line(t, n)).collect()
    };
    div()
        .id("recent")
        .w(px(460.))
        .max_h(px(420.))
        .flex()
        .flex_col()
        .rounded(px(10.))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .shadow(vec![super::shadow(t)])
        .occlude()
        .child(head)
        .child(
            div()
                .id("recent-lines")
                .flex()
                .flex_col()
                .overflow_y_scroll()
                .children(body),
        )
        .into_any_element()
}

impl Render for ToastsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let motion = Motion::of(cx);
        let (toasts, open, lines) = {
            let a = self.acts.read(cx);
            let lines: Vec<Notice> = if a.recent {
                a.notices.recent().cloned().collect()
            } else {
                Vec::new()
            };
            (a.notices.toasts().to_vec(), a.recent, lines)
        };
        if toasts.is_empty() && !open {
            return div().into_any_element();
        }
        let mut stack = div()
            .absolute()
            .right(px(20.))
            .bottom(px(46.))
            .flex()
            .flex_col()
            .items_end()
            .gap(px(8.))
            .font_family(UI_FAMILY)
            .text_size(px(13.))
            .line_height(px(19.));
        if open {
            stack = stack.child(recent(&t, &lines, cx));
        }
        stack
            .children(toasts.iter().map(|x| toast(&t, x, motion, cx)))
            .into_any_element()
    }
}

/// A small × that does `then`.
fn close_x(
    t: &Tokens,
    id: gpui::ElementId,
    cx: &mut Context<ToastsView>,
    then: impl Fn(&mut ToastsView, &mut Context<ToastsView>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|d| d.bg(t.sel))
        .child(icon("icons/close.svg", 12., t.dim))
        .on_click(cx.listener(move |this, _, _, cx| then(this, cx)))
}
