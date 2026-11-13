//! The checks before Up, when something is in the way (the PreFlight board; Go `components/preflight.tsx`): each check
//! with its ways out, and nothing started yet. With nothing red this sheet never shows. Keys (context `Sheet`): ⏎
//! takes the first way out offered as the default, `a` starts anyway, Esc cancels.

use doze_desk::act::{Fix, FixKind, Tone};
use gpui::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, Render, Window,
    div, prelude::*, px,
};

use crate::act::{Check, Then};
use crate::actions::{Confirm, GoAnyway};
use crate::kit::{Btn, btn, s};
use crate::theme::{Tokens, tokens, ty};
use crate::widgets::MONO_FAMILY;

/// What the sheet ends with.
#[derive(Debug, Clone)]
pub(crate) enum PreflightEvent {
    Cancel,
    Anyway,
    Fix(FixKind),
}

/// The sheet.
pub(crate) struct PreflightView {
    pub(crate) check: Check,
    focus: FocusHandle,
}

impl EventEmitter<PreflightEvent> for PreflightView {}

impl Focusable for PreflightView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PreflightView {
    pub(crate) fn new(check: Check, window: &mut Window, cx: &mut Context<Self>) -> PreflightView {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        PreflightView { check, focus }
    }

    fn first_fix(&self) -> Option<FixKind> {
        self.check
            .found
            .checks
            .iter()
            .flat_map(|c| &c.fixes)
            .find(|f| f.primary)
            .map(|f| f.kind.clone())
    }

    fn fix_button(t: &Tokens, id: (usize, usize), f: &Fix, cx: &mut Context<Self>) -> AnyElement {
        let kind = if f.primary {
            Btn::Primary
        } else if matches!(f.kind, FixKind::Edit(_)) {
            Btn::Quiet
        } else {
            Btn::Plain
        };
        let k = f.kind.clone();
        let el_id = gpui::SharedString::from(format!("fix-{}-{}", id.0, id.1));
        btn(
            t,
            el_id,
            &f.label,
            "",
            kind,
            Some(Box::new(cx.listener(move |_, _, _, cx| {
                cx.emit(PreflightEvent::Fix(k.clone()))
            }))),
        )
        .h(px(26.))
        .px(px(9.))
        // The way ⏎ takes wears the focus ring, as Go's focused button did.
        .when(f.primary, |b| b.shadow(vec![super::scrim::ring(t.accent)]))
        .into_any_element()
    }

    fn line(&self, t: &Tokens, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(c) = self.check.found.checks.get(i) else {
            return div().into_any_element();
        };
        let (glyph, color) = match c.tone {
            Tone::Ok => ("✓", t.good),
            Tone::Bad => ("✕", t.bad),
            Tone::Warn => ("!", t.warn),
        };
        let fixes: Vec<AnyElement> = c
            .fixes
            .iter()
            .enumerate()
            .map(|(j, f)| Self::fix_button(t, (i, j), f, cx))
            .collect();
        let ok = c.tone == Tone::Ok;
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .font_weight(if ok { ty::MEDIUM } else { ty::SEMIBOLD })
                    .text_color(if ok { t.text } else { t.bright })
                    .child(s(&c.title)),
            );
        let dim = |text: &str| div().text_size(px(ty::SM)).text_color(t.dim).child(s(text));
        if !c.detail.is_empty() {
            let d = dim(&c.detail);
            body = body.child(if c.mono {
                d.font_family(MONO_FAMILY).text_size(px(ty::XS_UP))
            } else {
                d
            });
        }
        if !c.note.is_empty() {
            body = body.child(dim(&c.note));
        }
        if !fixes.is_empty() {
            body = body.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .mt(px(6.))
                    .children(fixes),
            );
        }
        div()
            .flex()
            .items_start()
            .gap(px(10.))
            // Go's `.pf .row`: 12 px above and below, so each check reads as its own.
            .py(px(12.))
            .border_t_1()
            .border_color(t.line)
            .child(
                div()
                    .w(px(20.))
                    .flex_none()
                    .flex()
                    .justify_center()
                    .font_weight(ty::BOLD)
                    .text_color(color)
                    .child(glyph),
            )
            .child(body)
            .into_any_element()
    }
}

impl Render for PreflightView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let p = &self.check.found;
        let bad = p.checks.iter().filter(|c| c.tone == Tone::Bad).count();
        let instance = matches!(self.check.then, Then::Verb(..));
        let title = format!(
            "Before {} starts: {}",
            p.stack,
            if bad == 1 {
                "one thing is in the way".to_owned()
            } else {
                format!("{bad} things are in the way")
            }
        );
        let lead = if instance {
            "doze looks before it starts a service. When all is well there is no sheet: it just starts."
        } else {
            "doze looks before every Up. When all is well there is no sheet: the switch flips and the stack starts."
        };
        let lines: Vec<AnyElement> = (0..p.checks.len()).map(|i| self.line(&t, i, cx)).collect();
        super::scrim::panel(&t, 640., 10., title)
            .id("preflight")
            .key_context("Sheet")
            .track_focus(&self.focus)
            .on_action(
                cx.listener(|this, _: &Confirm, _, cx| match this.first_fix() {
                    Some(k) => cx.emit(PreflightEvent::Fix(k)),
                    None => cx.emit(PreflightEvent::Anyway),
                }),
            )
            .on_action(cx.listener(|_, _: &GoAnyway, _, cx| cx.emit(PreflightEvent::Anyway)))
            .child(
                div()
                    .text_size(px(ty::SM_UP))
                    .text_color(t.dim)
                    .mt(px(-4.))
                    .child(lead),
            )
            .child(div().flex().flex_col().children(lines))
            .child(self.foot(&t, instance, cx))
    }
}

impl PreflightView {
    /// "nothing has started yet", Up anyway, Cancel.
    fn foot(&self, t: &Tokens, instance: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let anyway = if instance {
            "Start anyway"
        } else {
            "Up anyway"
        };
        let go = Box::new(cx.listener(|_, _, _, cx| cx.emit(PreflightEvent::Anyway)));
        let cancel = Box::new(cx.listener(|_, _, _, cx| cx.emit(PreflightEvent::Cancel)));
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .pt(px(4.))
            .child(
                div()
                    .flex_1()
                    .text_size(px(ty::SM))
                    .text_color(t.dim)
                    .child("nothing has started yet"),
            )
            .child(btn(t, "pf-anyway", anyway, "a", Btn::Quiet, Some(go)))
            .child(btn(
                t,
                "pf-cancel",
                "Cancel",
                "esc",
                Btn::Plain,
                Some(cancel),
            ))
    }
}
