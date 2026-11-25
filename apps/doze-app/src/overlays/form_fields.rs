//! A form's fields as drawn: the label (and `required`), the box for its kind, the help under it, and the error the
//! app or the module found, under the field it is about.

use doze_desk::page::form::kind;
use gpui::{AnyElement, Context, Div, SharedString, Window, div, prelude::*, px};
use gpui_component::input::{Input, Textarea};

use super::{Field, FormView};
use crate::kit::s;
use crate::theme::{Tokens, ty};
use crate::widgets::Mono as _;

/// What an empty box says: a number's range, else what the field is.
pub(super) fn placeholder(f: &doze_control::FieldView) -> String {
    match (f.kind.as_str(), f.min, f.max) {
        (kind::NUMBER, Some(lo), Some(hi)) => format!("{lo} to {hi}"),
        (kind::NUMBER, Some(lo), None) => format!("{lo} or more"),
        (kind::NUMBER, None, Some(hi)) => format!("at most {hi}"),
        (kind::NUMBER, None, None) => "a whole number".to_owned(),
        _ => String::new(),
    }
}

impl FormView {
    /// One field: its label (and whether it is required), its box, its help, its error.
    pub(super) fn field(
        &self,
        t: &Tokens,
        i: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let f = self.form.fields().get(i)?;
        let error = self.form.error_of(&f.id).map(str::to_owned);
        let label = if f.label.is_empty() { &f.id } else { &f.label };
        let input = self.input(t, i, error.is_some(), window, cx)?;
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(head(t, label, f.required))
                .child(input)
                .when(!f.help.is_empty() && error.is_none(), |d| {
                    d.child(
                        div()
                            .text_size(px(ty::XS))
                            .text_color(t.dim)
                            .child(s(&f.help)),
                    )
                })
                .children(error.map(|e| {
                    crate::motion::settle(
                        "form-error",
                        div().text_size(px(ty::XS)).text_color(t.bad).child(s(&e)),
                        160,
                    )
                }))
                .into_any_element(),
        )
    }

    /// Field `i`'s box: a line (a number's narrower), lines for pairs, a switch, or its choices.
    fn input(
        &self,
        t: &Tokens,
        i: usize,
        wrong: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let f = self.form.fields().get(i)?;
        let error = wrong.then_some(());
        Some(match self.boxes.get(i)? {
            Field::Line(b) => {
                let focused = crate::widgets::has_keys(b, window, cx);
                let frame = crate::widgets::framed_input(
                    t,
                    &format!("form-{}", f.id),
                    focused,
                    Input::new(b),
                )
                .h(px(32.))
                .when(f.kind == kind::NUMBER, |d| {
                    d.font_features(ty::tabular()).w(px(180.))
                })
                .when(error.is_some(), |d| d.border_color(t.bad));
                frame.into_any_element()
            }
            Field::Lines(a) => div()
                .rounded(px(7.))
                .border_1()
                .border_color(if error.is_some() { t.bad } else { t.line2 })
                .bg(t.well)
                .px(px(8.))
                .py(px(4.))
                .mono_face()
                .text_size(px(ty::SM))
                .child(Textarea::new(a).appearance(false))
                .into_any_element(),
            Field::Pressed if f.kind == kind::BOOL => self.switch(t, i, cx),
            Field::Pressed => self.choices(t, i, cx),
        })
    }

    /// On/off: the app's switch and the word beside it.
    fn switch(&self, t: &Tokens, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let on = self.form.value(i) == "true";
        let id = format!(
            "form-bool-{}",
            self.form.fields().get(i).map_or("", |f| f.id.as_str())
        );
        div()
            .id(SharedString::from(id.clone()))
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor_pointer()
            .child(crate::widgets::switch(t, &id, on, false, false))
            .child(div().text_size(px(ty::SM)).text_color(t.dim).child(if on {
                "On"
            } else {
                "Off"
            }))
            .on_click(cx.listener(move |v, _, _, cx| {
                v.form.flip(i);
                cx.notify();
            }))
            .into_any_element()
    }

    /// A choice: its options side by side, the one chosen lit.
    fn choices(&self, t: &Tokens, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(f) = self.form.fields().get(i) else {
            return div().into_any_element();
        };
        let now = self.form.value(i).to_owned();
        let opts = f.choices.iter().enumerate().map(|(j, c)| {
            let on = *c == now;
            let c2 = c.clone();
            div()
                .id(SharedString::from(format!("form-choice-{}-{j}", f.id)))
                .flex()
                .items_center()
                .h(px(28.))
                .px(px(12.))
                .rounded(px(6.))
                .text_size(px(ty::SM))
                .font_weight(if on { ty::SEMIBOLD } else { ty::MEDIUM })
                .text_color(if on { t.bright } else { t.dim })
                .when(on, |d| d.bg(t.panel).shadow(t.lift()))
                .when(!on, |d| d.hover(|s| s.text_color(t.text)))
                .cursor_pointer()
                .child(s(c))
                .on_click(cx.listener(move |v, _, _, cx| {
                    v.form.set(i, &c2);
                    cx.notify();
                }))
        });
        div()
            .flex()
            .flex_wrap()
            .gap(px(2.))
            .p(px(2.))
            .rounded(px(8.))
            .bg(t.well)
            .border_1()
            .border_color(t.line)
            .self_start()
            .children(opts)
            .into_any_element()
    }
}

/// A field's label, and `required` beside it when it is.
fn head(t: &Tokens, label: &str, required: bool) -> Div {
    div()
        .flex()
        .items_baseline()
        .gap(px(6.))
        .child(
            div()
                .text_size(px(ty::SM_UP))
                .font_weight(ty::SEMIBOLD)
                .text_color(t.bright)
                .child(s(label)),
        )
        .when(required, |d| {
            d.child(
                div()
                    .text_size(px(ty::XS))
                    .text_color(t.dim)
                    .child("required"),
            )
        })
}
