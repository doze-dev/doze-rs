//! A confirmation over the dimmed window (the ConfirmSleep, Dialogs and Reset boards; Go `components/dialog.tsx`), in
//! doze-desk's words: the question, who is using it, what happens, the buttons in the dash's order, the foot. It owns
//! the keyboard while it shows (key context `Dialog`): ⏎ presses the highlighted button, Esc cancels, ←→ and Tab move
//! between the buttons, `y` and `n` answer a restart. A reset waits for its name and defaults to Cancel; typing the
//! name moves the default onto the reset. It enters with the overlays' motion (180 ms) and the page stays under it.

use doze_desk::act::{Button, Choosing, Pressed};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, Render, SharedString, Window, div, prelude::*, px,
};
use gpui_component::input::{Input, InputEvent, InputState};

use crate::act::Ask;
use crate::actions::{Confirm, DialogNo, DialogYes, MoveLeft, MoveRight};
use crate::kit::{mono, s, tone};
use crate::theme::{Tokens, alpha, mix, tokens, ty};
use crate::widgets::{Mono as _, UI_FAMILY, dot, kbd_plain};

/// What the dialog ends with.
#[derive(Debug, Clone)]
pub(crate) enum DialogEvent {
    Cancel,
    Chose(Button, String),
}

/// The dialog.
pub(crate) struct DialogView {
    pub(crate) ask: Ask,
    pub(crate) c: Choosing,
    input: Option<Entity<InputState>>,
    focus: FocusHandle,
}

impl EventEmitter<DialogEvent> for DialogView {}

impl Focusable for DialogView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DialogView {
    pub(crate) fn new(ask: Ask, window: &mut Window, cx: &mut Context<Self>) -> DialogView {
        let c = Choosing::new(ask.dialog.clone());
        let input = c.has_box().then(|| {
            let hint = ask.dialog.hint.clone();
            let i = cx.new(|cx| InputState::new(window, cx).placeholder(hint));
            cx.subscribe(&i, |this: &mut DialogView, i, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    let text = i.read(cx).value().to_string();
                    this.c.type_text(&text);
                    cx.notify();
                }
            })
            .detach();
            i
        });
        let focus = cx.focus_handle();
        match &input {
            Some(i) => i.update(cx, |i, cx| i.focus(window, cx)),
            None => window.focus(&focus, cx),
        }
        DialogView {
            ask,
            c,
            input,
            focus,
        }
    }

    /// Types `text` into the box, as the keys would.
    #[cfg_attr(not(feature = "capture"), allow(dead_code))] // WHY: shots type into the dialog
    pub(crate) fn type_in(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(i) = &self.input {
            i.update(cx, |i, cx| i.set_value(text.to_owned(), window, cx));
        }
        self.c.type_text(text);
        cx.notify();
    }

    fn pressed(&mut self, p: Pressed, cx: &mut Context<Self>) {
        match p {
            Pressed::Nothing => {}
            Pressed::Cancel => cx.emit(DialogEvent::Cancel),
            Pressed::Run(b, text) => cx.emit(DialogEvent::Chose(b, text)),
        }
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        self.c.step(by);
        cx.notify();
    }
}

/// The rows of "what happens", in the well.
fn rows(t: &Tokens, v: &DialogView) -> Option<AnyElement> {
    let rows = &v.c.dialog.rows;
    if rows.is_empty() {
        return None;
    }
    let lines = rows.iter().enumerate().map(|(i, r)| {
        // Go's `.kv`: the dot and the word centred against a value that wraps.
        let line = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .py(px(8.))
            .when(i > 0, |d| d.border_t_1().border_color(t.line));
        if r.k.is_empty() {
            return line.child(div().text_color(t.dim).child(s(&r.v)));
        }
        line.child(dot(t.good))
            .child(
                div()
                    .w(px(120.))
                    .flex_none()
                    .font_weight(ty::SEMIBOLD)
                    .text_color(t.bright)
                    .child(s(&r.k)),
            )
            .child(div().flex_1().min_w_0().text_color(t.dim).child(s(&r.v)))
    });
    Some(
        div()
            .flex()
            .flex_col()
            .rounded(px(10.))
            .border_1()
            .border_color(t.line)
            .bg(t.well)
            .children(lines)
            .into_any_element(),
    )
}

/// A reset's facts: the key down the left, the words in their tone, the folder in mono.
fn facts(t: &Tokens, v: &DialogView) -> Option<AnyElement> {
    let facts = &v.c.dialog.facts;
    if facts.is_empty() {
        return None;
    }
    let lines = facts.iter().map(|f| {
        let words = f.spans.iter().map(|sp| {
            let d = div().child(s(&sp.text));
            match (sp.strong, sp.mono) {
                (_, true) => d.mono_face().font_weight(ty::SEMIBOLD).text_color(t.bright),
                (true, _) => d.font_weight(ty::SEMIBOLD).text_color(t.bright),
                _ => d.text_color(tone(t, &f.tone)),
            }
        });
        // Go's `.facts`: the word centred against what may wrap to two lines.
        div()
            .flex()
            .items_center()
            .child(
                div()
                    .w(px(110.))
                    .flex_none()
                    .text_color(t.dim)
                    .child(s(&f.k)),
            )
            .child(div().flex().flex_wrap().flex_1().min_w_0().children(words))
    });
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .children(lines)
            .into_any_element(),
    )
}

/// The sentence, its names and figures bright.
fn sentence(t: &Tokens, v: &DialogView) -> Option<AnyElement> {
    let spans = &v.c.dialog.sentence;
    if spans.is_empty() {
        return None;
    }
    let mut text = String::new();
    let mut runs = Vec::new();
    for sp in spans {
        let start = text.len();
        text.push_str(&sp.text);
        if sp.strong || sp.mono {
            let style = gpui::HighlightStyle {
                color: Some(t.bright),
                font_weight: Some(ty::SEMIBOLD),
                ..Default::default()
            };
            runs.push((start..text.len(), style));
        }
    }
    Some(
        div()
            .text_color(t.text)
            .child(gpui::StyledText::new(text).with_highlights(runs))
            .into_any_element(),
    )
}

/// The box: a reset's "Type db to confirm", or an action's input.
fn input(t: &Tokens, v: &DialogView) -> Option<AnyElement> {
    let i = v.input.as_ref()?;
    let field = div()
        .h(px(36.))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(8.))
        .border_1()
        .border_color(mix(t.accent, t.line2, 45.))
        .bg(t.well)
        .child(
            div().flex_1().child(
                Input::new(i)
                    .appearance(false)
                    .bordered(false)
                    .focus_bordered(false)
                    .px_0()
                    .text_size(px(ty::BASE))
                    .text_color(t.bright)
                    .when(!v.c.dialog.type_name.is_empty(), |x| x.mono_face()),
            ),
        );
    let name = &v.c.dialog.type_name;
    if name.is_empty() {
        return Some(field.into_any_element());
    }
    let prompt = div()
        .flex()
        .items_center()
        .gap(px(4.))
        .text_size(px(ty::SM_UP))
        .text_color(t.dim)
        .child("Type")
        .child(
            mono(SharedString::from(name.clone()), ty::SM_UP, t.bright).font_weight(ty::SEMIBOLD),
        )
        .child("to confirm");
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(prompt)
            .child(field)
            .into_any_element(),
    )
}

/// The foot, "From a script:" with its command in mono.
fn foot(t: &Tokens, v: &DialogView) -> Option<AnyElement> {
    let foot = &v.c.dialog.foot;
    if foot.is_empty() {
        return None;
    }
    let lines = foot
        .iter()
        .map(|f| match f.strip_prefix("From a script: ") {
            Some(cmd) => div().flex().gap(px(4.)).child("From a script:").child(mono(
                SharedString::from(cmd.to_owned()),
                ty::SM,
                t.bright,
            )),
            None => div().child(s(f)),
        });
    Some(
        div()
            .flex()
            .flex_col()
            .text_size(px(ty::SM))
            .text_color(t.dim)
            .children(lines)
            .into_any_element(),
    )
}

impl DialogView {
    fn button(&self, t: &Tokens, i: usize, b: &Button, cx: &mut Context<Self>) -> AnyElement {
        let at = i == self.c.at;
        let on = self.c.enabled(i);
        let (bg, border, fg) = if b.danger {
            (t.bad, t.bad, t.ink)
        } else if b.primary {
            (t.accent, t.accent, t.ink)
        } else {
            (t.panel, if at { t.accent } else { t.line2 }, t.text)
        };
        let key = if b.cancel {
            "esc"
        } else if at {
            "⏎"
        } else {
            ""
        };
        let key_c = if b.danger || b.primary {
            alpha(t.ink, 0.7)
        } else {
            t.dim
        };
        div()
            .id(("dialog-button", i))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .h(px(30.))
            .px(px(12.))
            .rounded(px(7.))
            .border_1()
            .border_color(border)
            .bg(bg)
            .text_size(px(ty::SM))
            .font_weight(if b.primary || b.danger {
                ty::SEMIBOLD
            } else {
                ty::MEDIUM
            })
            .text_color(fg)
            // The highlighted button rings, Cancel too (Go's focus ring); not while the name is being typed.
            .when(at && on && !self.c.has_box(), |d| {
                d.shadow(vec![super::scrim::ring(if b.danger {
                    t.bad
                } else {
                    t.accent
                })])
            })
            .when(!on, |d| d.opacity(0.6))
            .when(on, |d| d.cursor_pointer())
            .child(s(&b.label))
            .when(!key.is_empty(), |d| {
                d.child(kbd_plain(key, ty::XS, key_c, ty::MEDIUM))
            })
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.c.at != i {
                    this.c.hover(i);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                let p = this.c.press(i);
                this.pressed(p, cx);
            }))
            .into_any_element()
    }
}

impl Render for DialogView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let d = self.c.dialog.clone();
        let wide = d.buttons.len() >= 4;
        let buttons: Vec<AnyElement> = d
            .buttons
            .iter()
            .enumerate()
            .map(|(i, b)| self.button(&t, i, b, cx))
            .collect();
        div()
            .id("dialog")
            .key_context("Dialog")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &MoveRight, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &MoveLeft, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &Confirm, _, cx| {
                let p = this.c.enter();
                this.pressed(p, cx);
            }))
            .on_action(cx.listener(|this, _: &DialogYes, _, cx| {
                let p = this.c.yes();
                this.pressed(p, cx);
            }))
            .on_action(cx.listener(|this, _: &DialogNo, _, cx| {
                let p = this.c.no();
                this.pressed(p, cx);
            }))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(if wide { 620. } else { 520. }))
            .flex()
            .flex_col()
            .gap(px(12.))
            .px(px(22.))
            .py(px(20.))
            .relative()
            .child(super::scrim::frame(
                &t,
                if d.danger { t.bad } else { t.line2 },
            ))
            .font_family(UI_FAMILY)
            .text_size(px(ty::BASE))
            .line_height(px(ty::LH_19))
            .child(
                div()
                    .text_size(px(ty::LG))
                    .font_weight(ty::HEADING)
                    .text_color(t.bright)
                    .child(s(&d.title)),
            )
            .children(sentence(&t, self))
            .children(rows(&t, self))
            .children(facts(&t, self))
            .children(input(&t, self))
            .children(foot(&t, self))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_end()
                    .gap(px(8.))
                    .mt(px(4.))
                    .children(buttons),
            )
    }
}

#[cfg(all(test, feature = "capture"))]
mod tests;
