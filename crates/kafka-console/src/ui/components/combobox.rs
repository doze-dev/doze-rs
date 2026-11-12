//! Combobox: a Select you can type into. It filters as you type, says so when nothing matches, and may take free text
//! (`create`). Without a script: a `<select>` (a closed list) or a text field with a `<datalist>` (free text).

use maud::{Markup, html};

use super::{Choice, Icon, icon, options, shown, alpine};

/// A Combobox. For free text (`create`) the form control is a text field named `name`; otherwise a `<select>`.
#[derive(Debug, Clone, Copy)]
pub struct Combobox<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub label: &'a str,
    pub choices: &'a [Choice<'a>],
    pub value: &'a str,
    pub placeholder: &'a str,
    /// Free text is a value: typing a name that is not in the list offers "Use ..." and keeps it.
    pub create: bool,
    pub auto_submit: bool,
    pub class: &'a str,
}

impl<'a> Combobox<'a> {
    #[must_use]
    pub const fn new(id: &'a str, name: &'a str, label: &'a str, choices: &'a [Choice<'a>], value: &'a str) -> Self {
        Self { id, name, label, choices, value, placeholder: "Search", create: false, auto_submit: false, class: "" }
    }
}

/// The Combobox.
#[must_use]
pub fn combobox(c: &Combobox<'_>) -> Markup {
    alpine(combobox_html(c))
}

fn combobox_html(c: &Combobox<'_>) -> Markup {
    let list_id = format!("{}-list", c.id);
    let (now, chosen) = shown(c.choices, c.value, "");
    let datalist_id = format!("{}-dl", c.id);
    html! {
        div class={"dz dz-combobox " (c.class)} x-data="dzCombobox" x-on:keydown="key" x-on:click="click" x-on:mousemove="hover" x-on:focusout="focusout" xo-click="away" xb-data-open="open" data-ui="combobox" data-create[c.create] data-name=(c.name) {
            @if c.create {
                input class="dz-native" type="text" id=(c.id) name=(c.name) value=(c.value) list=(datalist_id) autocomplete="off" aria-label=(c.label) data-autosubmit[c.auto_submit];
                datalist id=(datalist_id) { @for ch in c.choices { option value=(ch.value) { (ch.label) } } }
            } @else {
                select class="dz-native" id=(c.id) name=(c.name) aria-label=(c.label) data-autosubmit[c.auto_submit] {
                    @for ch in c.choices { option value=(ch.value) selected[ch.value == c.value] { (ch.label) } }
                    @if !chosen && !c.value.is_empty() { option value=(c.value) selected { (c.value) } }
                }
            }
            div class="dz-field" x-ref="field" hidden {
                (icon(Icon::Search, "dz-lead"))
                input class="dz-input" x-ref="input" type="text" role="combobox" aria-autocomplete="list" aria-haspopup="listbox" aria-expanded="false" xb-aria-expanded="open"
                    aria-controls=(list_id) autocomplete="off" spellcheck="false" placeholder=(c.placeholder) value=(if c.create { c.value } else { now }) x-on:input="input";
                button type="button" class="dz-chev-btn dz-chev" tabindex="-1" aria-hidden="true" { (icon(Icon::Chevron, "")) }
            }
            div class="dz-pop" role="listbox" id=(list_id) aria-label=(c.label) x-ref="list" x-show="open" xa-bottom-start-4="$refs.field" style="display:none" {
                (options(c.id, c.choices, c.value))
                div class="dz-opt" role="option" id=(format!("{}-create", c.id)) data-create-row data-value="" hidden {
                    (icon(Icon::Check, "dz-check"))
                    span class="dz-opt-t" { span class="dz-opt-label" { "Use " span class="dz-create-text" {} } }
                }
                div class="dz-empty" hidden { "No match" }
            }
        }
    }
}
