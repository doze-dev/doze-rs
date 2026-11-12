//! Select: a single choice. A real `<select>` is the form control and the no-script control; the script hides it and shows a
//! button (`role=combobox`) and a list (`role=listbox`) in the shadcn look, and keeps the `<select>`'s value, so a form submits
//! the same either way.

use maud::{Markup, html};

use super::{Choice, Icon, alpine, icon, options, shown};

/// A Select. `id` and `name` are the `<select>`'s; `label` is its accessible name when no `<label for=id>` is drawn.
#[derive(Debug, Clone, Copy)]
pub struct Select<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub label: &'a str,
    pub choices: &'a [Choice<'a>],
    pub value: &'a str,
    /// Shown when no choice has the value.
    pub placeholder: &'a str,
    /// A change submits the form it is in (a filter).
    pub auto_submit: bool,
    pub disabled: bool,
    /// Extra classes on the root (`chip` for the Live filters).
    pub class: &'a str,
    /// The `data-ui` kind of the button (the boards name a filter chip's button `filter`).
    pub ui: Option<&'a str>,
}

impl<'a> Select<'a> {
    #[must_use]
    pub const fn new(
        id: &'a str,
        name: &'a str,
        label: &'a str,
        choices: &'a [Choice<'a>],
        value: &'a str,
    ) -> Self {
        Self {
            id,
            name,
            label,
            choices,
            value,
            placeholder: "Choose",
            auto_submit: false,
            disabled: false,
            class: "",
            ui: None,
        }
    }
}

/// The Select.
#[must_use]
pub fn select(s: &Select<'_>) -> Markup {
    alpine(select_html(s))
}

fn select_html(s: &Select<'_>) -> Markup {
    let (now, chosen) = shown(s.choices, s.value, s.placeholder);
    let list_id = format!("{}-list", s.id);
    html! {
        div class={"dz dz-select " (s.class)} x-data="dzSelect" x-on:keydown="key" x-on:click="click" x-on:mousemove="hover" x-on:focusout="focusout" xo-click="away" xb-data-open="open" data-ui="select" data-name=(s.name) {
            // The control without a script, and the one a form reads with it.
            select class="dz-native" id=(s.id) name=(s.name) aria-label=(s.label) data-autosubmit[s.auto_submit] disabled[s.disabled] {
                // With nothing chosen and no "all" choice the browser would pick the first option: say "nothing" instead.
                @if s.value.is_empty() && !s.choices.iter().any(|c| c.value.is_empty()) { option value="" selected hidden { (s.placeholder) } }
                @for c in s.choices { option value=(c.value) selected[c.value == s.value] disabled[c.disabled] { (c.label) } }
                @if !chosen && !s.value.is_empty() { option value=(s.value) selected { (s.value) } }
            }
            button type="button" class="dz-trigger" x-ref="trigger" role="combobox" aria-haspopup="listbox" aria-expanded="false" xb-aria-expanded="open" aria-controls=(list_id)
                data-placeholder=(s.placeholder) data-ui=[s.ui] disabled[s.disabled] hidden {
                span class={"dz-value" @if !chosen { " muted" }} { (now) }
                (icon(Icon::Chevron, "dz-chev"))
            }
            div class="dz-pop" role="listbox" id=(list_id) aria-label=(s.label) x-ref="list" x-show="open" xa-bottom-start-4="$refs.trigger" style="display:none" {
                (options(s.id, s.choices, s.value))
            }
        }
    }
}
