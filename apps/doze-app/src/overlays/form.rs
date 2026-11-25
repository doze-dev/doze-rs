//! An action's form over the dimmed page (module protocol minor 2): its typed fields (text, secret, a whole number in
//! its range, a choice, on/off, key=value lines), the defaults filled in, help under the field it explains, and an
//! error under the field it is about, the app's own check first and then the module's. Every field is sent, blanks
//! included, with `form` set; the form stays up until the module says it is done, and says why when it is not. Esc
//! (the window's) or a press outside cancels; ⏎ in a box presses the action.

use doze_desk::page::console::Action;
use doze_desk::page::form::{Form, kind};
use doze_desk::{FormOutcome, Hub, StackKey};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, Render, Window, div, prelude::*, px,
};
use gpui_component::input::{InputEvent, InputState, TextareaState};

use crate::kit::{Btn, btn, s};
use crate::theme::{Tokens, tokens, ty};
use crate::widgets::UI_FAMILY;

/// What the form ends with.
#[derive(Debug, Clone)]
pub(crate) enum FormEvent {
    Cancel,
    /// The module ran it and said this.
    Done(String),
}

#[path = "form_fields.rs"]
mod fields;
use fields::placeholder;

/// A field's box.
enum Field {
    Line(Entity<InputState>),
    Lines(Entity<TextareaState>),
    /// A choice or on/off: pressed, not typed.
    Pressed,
}

/// The form.
pub(crate) struct FormView {
    pub(crate) key: StackKey,
    hub: Hub,
    pub(crate) form: Form,
    boxes: Vec<Field>,
    focus: FocusHandle,
}

impl EventEmitter<FormEvent> for FormView {}

impl Focusable for FormView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl FormView {
    pub(crate) fn new(
        (hub, key): (Hub, StackKey),
        instance: &str,
        action: &Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> FormView {
        let form = Form::new(instance, action);
        let mut boxes = Vec::new();
        for (i, f) in form.fields().iter().enumerate() {
            let v = form.value(i).to_owned();
            let b = match f.kind.as_str() {
                kind::BOOL | kind::CHOICE => Field::Pressed,
                kind::PAIRS => {
                    let a = cx.new(|cx| {
                        TextareaState::new(window, cx)
                            .auto_grow(2, 6)
                            .placeholder("key=value, one a line")
                    });
                    a.update(cx, |a, cx| a.set_value(v, window, cx));
                    cx.subscribe(&a, move |this: &mut FormView, a, ev: &InputEvent, cx| {
                        if matches!(ev, InputEvent::Change) {
                            let text = a.read(cx).value().to_string();
                            this.form.set(i, &text);
                            cx.notify();
                        }
                    })
                    .detach();
                    Field::Lines(a)
                }
                k => {
                    let hint = placeholder(f);
                    let secret = k == kind::SECRET;
                    let b =
                        cx.new(|cx| InputState::new(window, cx).placeholder(hint).masked(secret));
                    b.update(cx, |b, cx| b.set_value(v, window, cx));
                    cx.subscribe_in(
                        &b,
                        window,
                        move |this: &mut FormView, b, ev: &InputEvent, _, cx| match ev {
                            InputEvent::Change => {
                                let text = b.read(cx).value().to_string();
                                this.form.set(i, &text);
                                cx.notify();
                            }
                            InputEvent::PressEnter { .. } => this.submit(cx),
                            _ => {}
                        },
                    )
                    .detach();
                    Field::Line(b)
                }
            };
            boxes.push(b);
        }
        let focus = cx.focus_handle();
        match boxes.iter().find_map(|b| match b {
            Field::Line(i) => Some(i.clone()),
            _ => None,
        }) {
            Some(i) => i.update(cx, |i, cx| i.focus(window, cx)),
            None => window.focus(&focus, cx),
        }
        FormView {
            key,
            hub,
            form,
            boxes,
            focus,
        }
    }

    /// The action pressed: checked here first, then sent with every field's value.
    pub(crate) fn submit(&mut self, cx: &mut Context<Self>) {
        if self.form.running {
            return;
        }
        let Some(values) = self.form.submit() else {
            cx.notify();
            return;
        };
        self.form.running = true;
        cx.notify();
        let (hub, key) = (self.hub.clone(), self.key.clone());
        let (inst, id, resource) = (
            self.form.instance.clone(),
            self.form.action.id.clone(),
            self.form.action.resource.clone(),
        );
        let label = self.form.action.label.clone();
        cx.spawn(async move |this, cx| {
            let got = cx
                .background_executor()
                .spawn(async move { hub.admin_form(&key, &inst, (&id, &resource), values) })
                .await;
            let _ = this.update(cx, |v, cx| {
                match got {
                    Ok(FormOutcome::Done(said)) => {
                        let said = if said.is_empty() {
                            format!("{label} done")
                        } else {
                            said
                        };
                        v.form.running = false;
                        cx.emit(FormEvent::Done(said));
                    }
                    Ok(FormOutcome::Refused { message, field }) => v.form.refused(&message, &field),
                    Err(e) => v.form.refused(doze_display::humanise(&e.to_string()), ""),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Types `text` into field `id` (a shot).
    #[cfg(feature = "capture")]
    pub(crate) fn type_in(
        &mut self,
        id: &str,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(i) = self.form.fields().iter().position(|f| f.id == id) else {
            return;
        };
        match self.boxes.get(i) {
            Some(Field::Line(b)) => b.update(cx, |b, cx| b.set_value(text.to_owned(), window, cx)),
            Some(Field::Lines(b)) => b.update(cx, |b, cx| b.set_value(text.to_owned(), window, cx)),
            _ => {}
        }
        self.form.set(i, text);
        cx.notify();
    }
}

impl FormView {
    /// The action's name, and what it acts on.
    fn title(&self, t: &Tokens) -> gpui::Div {
        let a = &self.form.action;
        let on = if a.resource.is_empty() {
            self.form.instance.clone()
        } else {
            format!("{} · {}", a.resource, self.form.instance)
        };
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(ty::LG))
                    .font_weight(ty::HEADING)
                    .text_color(t.bright)
                    .child(s(&a.label)),
            )
            .child(div().text_size(px(ty::SM)).text_color(t.dim).child(s(&on)))
    }

    /// Cancel, and the action named (red when it deletes), "Running…" while the module works.
    fn buttons(&self, t: &Tokens, cx: &mut Context<Self>) -> gpui::Div {
        let a = &self.form.action;
        let running = self.form.running;
        let me = cx.entity().downgrade();
        let go: crate::kit::Click = Box::new(move |_, _, cx| {
            let _ = me.update(cx, |v, cx| v.submit(cx));
        });
        let me = cx.entity().downgrade();
        let cancel: crate::kit::Click = Box::new(move |_, _, cx| {
            let _ = me.update(cx, |_, cx| cx.emit(FormEvent::Cancel));
        });
        let label = if running {
            "Running…"
        } else {
            a.label.as_str()
        };
        let key = if running { "" } else { "⏎" };
        let mut press = btn(t, "form-go", label, key, Btn::Primary, Some(go));
        if a.destructive {
            press = press.bg(t.bad).border_color(t.bad);
        }
        div()
            .flex()
            .justify_end()
            .items_center()
            .gap(px(8.))
            .child(btn(
                t,
                "form-cancel",
                "Cancel",
                "esc",
                Btn::Plain,
                Some(cancel),
            ))
            .child(press.when(running, |b| b.opacity(0.7)))
    }
}

impl Render for FormView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let danger = self.form.action.destructive;
        let fields: Vec<AnyElement> = (0..self.form.fields().len())
            .filter_map(|i| self.field(&t, i, window, cx))
            .collect();
        let general = self.form.general_error().map(str::to_owned);
        div()
            .id("form")
            .key_context("Form")
            .track_focus(&self.focus)
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(540.))
            .max_h(px(560.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .px(px(22.))
            .py(px(20.))
            .relative()
            .child(super::scrim::frame(
                &t,
                if danger { t.bad } else { t.line2 },
            ))
            .font_family(UI_FAMILY)
            .text_size(px(ty::BASE))
            .line_height(px(ty::LH_19))
            .child(self.title(&t))
            .child(
                div()
                    .id("form-fields")
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .overflow_y_scroll()
                    .children(fields),
            )
            .children(general.map(|g| {
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .bg(crate::theme::alpha(t.bad, 0.08))
                    .text_size(px(ty::SM))
                    .text_color(t.bad)
                    .child(s(&g))
            }))
            .child(self.buttons(&t, cx))
    }
}
