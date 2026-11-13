//! The start sheet (Go `StartSheet`): how a stopped stack starts some other way than last time. Tick the apps to work
//! on; what they need is shown starting with them, and modules nobody ticked still wake when something connects. Up
//! remembers the set, so next time "as last time" is this. Keys (context `Sheet`): ↑↓ move, space ticks, ⏎ starts
//! these, `e` everything, Esc cancels.

use doze_desk::StackKey;
use doze_desk::act::{StartSheet, StartWord};
use gpui::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, Render, Window,
    div, prelude::*, px,
};

use crate::actions::{Confirm, MoveDown, MoveUp, StartEverything, ToggleTick};
use crate::kit::{Btn, btn, s};
use crate::theme::{Tokens, tokens, ty};
use crate::widgets::{icon, kbd_plain, tracked};

/// What the sheet ends with.
#[derive(Debug, Clone)]
pub(crate) enum StartEvent {
    Cancel,
    /// Start these names.
    These(Vec<String>),
    Everything,
}

/// The sheet.
pub(crate) struct StartView {
    pub(crate) key: StackKey,
    pub(crate) sheet: StartSheet,
    focus: FocusHandle,
}

impl EventEmitter<StartEvent> for StartView {}

impl Focusable for StartView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl StartView {
    pub(crate) fn new(
        key: StackKey,
        sheet: StartSheet,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> StartView {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        StartView { key, sheet, focus }
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        let chosen = self.sheet.chosen();
        if !chosen.is_empty() {
            cx.emit(StartEvent::These(chosen));
        }
    }

    fn pick(&self, t: &Tokens, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let (Some(p), Some(on)) = (self.sheet.picks.get(i), self.sheet.ticked.get(i).copied())
        else {
            return div().into_any_element();
        };
        let at = i == self.sheet.at;
        let tick = div()
            .size(px(14.))
            .flex_none()
            .rounded(px(3.))
            .border_1()
            .border_color(if on { t.accent } else { t.dim })
            .when(on, |d| d.bg(t.accent))
            .flex()
            .items_center()
            .justify_center()
            .when(on, |d| d.child(icon("icons/check.svg", 11., t.ink)));
        div()
            .id(("pick", i))
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .py(px(9.))
            .cursor_pointer()
            .when(i > 0, |d| d.border_t_1().border_color(t.line))
            .when(at, |d| d.bg(t.pick))
            .child(tick)
            .child(
                div()
                    .w(px(120.))
                    .flex_none()
                    .font_weight(ty::SEMIBOLD)
                    .text_color(t.bright)
                    .child(s(&p.name)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(t.dim)
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(s(&p.needs)),
            )
            .when(p.last, |d| {
                d.child(
                    div()
                        .px(px(5.))
                        .rounded(px(3.))
                        .bg(crate::theme::mix(t.accent, t.panel, 30.))
                        .child(tracked("last time", ty::TAG, ty::BOLD, t.accent, 0.5)),
                )
            })
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.sheet.at != i {
                    this.sheet.at = i;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.sheet.toggle(i);
                cx.notify();
            }))
            .into_any_element()
    }

    fn also(&self, t: &Tokens) -> AnyElement {
        match self.sheet.also() {
            (_, true) => div()
                .text_color(t.warn)
                .child("Tick at least one, or start everything."),
            (Some(b), _) => div()
                .flex()
                .gap(px(4.))
                .text_color(t.text)
                .child("Also starts, because they are needed:")
                .child(
                    div()
                        .font_weight(ty::SEMIBOLD)
                        .text_color(t.bright)
                        .child(s(&b)),
                ),
            (None, _) => div()
                .text_color(t.dim)
                .child("Nothing else starts with them."),
        }
        .text_size(px(ty::SM))
        .into_any_element()
    }
}

impl Render for StartView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let picks: Vec<AnyElement> = (0..self.sheet.picks.len())
            .map(|i| self.pick(&t, i, cx))
            .collect();
        let buttons = self.buttons(&t, cx);
        super::scrim::panel(&t, 560., 12., format!("Start {}", self.sheet.stack))
            .id("start-sheet")
            .key_context("Sheet")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &MoveDown, _, cx| {
                this.sheet.step(1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &MoveUp, _, cx| {
                this.sheet.step(-1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleTick, _, cx| {
                let at = this.sheet.at;
                this.sheet.toggle(at);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Confirm, _, cx| this.start(cx)))
            .on_action(cx.listener(|_, _: &StartEverything, _, cx| cx.emit(StartEvent::Everything)))
            .child(div().text_color(t.text).child("Tick what you will work on. doze starts those and what they need, in order; the rest stays down until something connects to it."))
            .child(div().flex().flex_col().rounded(px(10.)).border_1().border_color(t.line).bg(t.well).overflow_hidden().children(picks))
            .child(self.also(&t))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .child(div().flex_1().flex().gap(px(4.)).text_size(px(ty::XS)).text_color(t.dim).child(kbd_plain("space", ty::XS, t.dim, ty::MEDIUM)).child("ticks"))
                    .children(buttons),
            )
    }
}

impl StartView {
    /// Cancel, Everything, Start these.
    fn buttons(&self, t: &Tokens, cx: &mut Context<Self>) -> [AnyElement; 3] {
        let word = match self.sheet.word() {
            StartWord::All => "Start all",
            StartWord::One => "Start",
            StartWord::These => "Start these",
        };
        let none = self.sheet.chosen().is_empty();
        let start = btn(
            t,
            "start-these",
            word,
            "⏎",
            Btn::Primary,
            Some(Box::new(cx.listener(|this, _, _, cx| this.start(cx)))),
        )
        .when(none, |b| b.opacity(0.45));
        let every = btn(
            t,
            "start-all",
            "Everything",
            "e",
            Btn::Plain,
            Some(Box::new(
                cx.listener(|_, _, _, cx| cx.emit(StartEvent::Everything)),
            )),
        );
        let cancel = btn(
            t,
            "start-cancel",
            "Cancel",
            "esc",
            Btn::Quiet,
            Some(Box::new(
                cx.listener(|_, _, _, cx| cx.emit(StartEvent::Cancel)),
            )),
        );
        [
            cancel.into_any_element(),
            every.into_any_element(),
            start.into_any_element(),
        ]
    }
}
