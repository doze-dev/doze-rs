//! MultiSelect: several choices from a list. The form control, and the no-script control, is a group of real checkboxes in a
//! `<details>`; the script hides it and shows a button (the selection, compactly), and a popup with a search field, "Select all"
//! and "Clear" and a list of options with checks (`role=listbox`, `aria-multiselectable`). The checkboxes stay in the form and
//! hold the state, so a form submits the same either way: one `name=value` per chosen option (`topic=a&topic=b`).

use maud::{Markup, html};

use super::{Choice, Icon, alpine, icon};

/// A MultiSelect. `name` is the repeated field every checkbox carries; `label` names the control ("Topics").
#[derive(Debug, Clone, Copy)]
pub struct MultiSelect<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub label: &'a str,
    /// The options; `Choice::count` is the rows each has, when the server knows.
    pub choices: &'a [Choice<'a>],
    /// The chosen values. One that no choice has stays a choice (with no count), so the page never lies about what it filters by.
    pub values: &'a [String],
    /// Shown when nothing is chosen ("All topics").
    pub placeholder: &'a str,
    /// What more than two chosen are called ("topics": `3 topics`).
    pub noun: &'a str,
    /// A change submits the form it is in (a filter), a moment after the last one.
    pub auto_submit: bool,
    pub class: &'a str,
    /// The `data-ui` kind of the button (the boards name a filter chip's button `filter`).
    pub ui: Option<&'a str>,
}

impl<'a> MultiSelect<'a> {
    #[must_use]
    pub const fn new(
        id: &'a str,
        name: &'a str,
        label: &'a str,
        choices: &'a [Choice<'a>],
        values: &'a [String],
    ) -> Self {
        Self {
            id,
            name,
            label,
            choices,
            values,
            placeholder: "All",
            noun: "items",
            auto_submit: false,
            class: "",
            ui: None,
        }
    }
}

/// What the button says of a selection: the placeholder, one label, two labels, then a count. The script writes the same words.
#[must_use]
pub fn summary(placeholder: &str, noun: &str, labels: &[&str]) -> String {
    match labels {
        [] => placeholder.to_owned(),
        [one] => (*one).to_owned(),
        [a, b] => format!("{a}, {b}"),
        more => format!("{} {noun}", more.len()),
    }
}

/// The MultiSelect.
#[must_use]
pub fn multiselect(m: &MultiSelect<'_>) -> Markup {
    alpine(multiselect_html(m))
}

/// The control without a script, and the one a form reads with it: a details of checkboxes, with a real submit button.
fn plain(m: &MultiSelect<'_>, all: &[Choice<'_>], now: &str) -> Markup {
    let on = |c: &Choice<'_>| m.values.iter().any(|v| v == c.value);
    html! {
        details class="dz-native dz-multi-plain" id=(m.id) {
            summary { (now) }
            fieldset class="dz-multi-checks" {
                legend class="sr-only" { (m.label) }
                @for (i, c) in all.iter().enumerate() {
                    label class="dz-multi-check" {
                        input type="checkbox" name=(m.name) value=(c.value) checked[on(c)] id=(format!("{}-c{i}", m.id));
                        span class="dz-multi-check-l" { (c.label) }
                        @if let Some(k) = c.count { span class="dz-count" { (k) } }
                    }
                }
                button type="submit" class="btn sm" data-noscript { "Apply" }
            }
        }
    }
}

fn multiselect_html(m: &MultiSelect<'_>) -> Markup {
    // The options, then the chosen values no option has.
    let mut all: Vec<Choice<'_>> = Vec::with_capacity(m.choices.len());
    for c in m.choices {
        // One option per value: a repeated one would be checked twice and submitted twice.
        if !all.iter().any(|a| a.value == c.value) {
            all.push(*c);
        }
    }
    for v in m.values {
        if !all.iter().any(|c| c.value == v) {
            all.push(Choice::new(v, v));
        }
    }
    let on = |c: &Choice<'_>| m.values.iter().any(|v| v == c.value);
    let chosen: Vec<&str> = all.iter().filter(|c| on(c)).map(|c| c.label).collect();
    let now = summary(m.placeholder, m.noun, &chosen);
    let list_id = format!("{}-list", m.id);
    let pop_id = format!("{}-pop", m.id);
    let n = chosen.len();
    let lower = m.noun;
    html! {
        div class={"dz dz-multi " (m.class) @if n > 0 { " on" }} x-data="dzMulti" x-on:keydown="key" x-on:click="click" x-on:mousemove="hover" x-on:mousedown="down" x-on:focusout="focusout" xo-click="away" xb-data-open="open"
            id=(format!("{}-ms", m.id)) data-ui="multiselect" data-name=(m.name) data-noun=(m.noun) data-placeholder=(m.placeholder) data-autosubmit[m.auto_submit] {
            (plain(m, &all, &now))
            div class="dz-multi-box" {
                button type="button" class={"dz-trigger dz-multi-trigger" @if n > 0 { " has" }} x-ref="trigger" aria-haspopup="listbox" aria-expanded="false" xb-aria-expanded="open" aria-controls=(pop_id)
                    aria-label=(format!("{}: {}", m.label, now)) title=(chosen.join(", ")) data-ui=[m.ui] hidden {
                    span class={"dz-value" @if n == 0 { " muted" }} { (now) }
                    (icon(Icon::Chevron, "dz-chev"))
                }
                button type="button" class="dz-multi-clear" x-ref="clear" x-show="has" data-clear aria-label=(format!("Clear {}", m.label.to_lowercase())) style="display:none" {
                    (icon(Icon::Close, ""))
                }
            }
            div class="dz-pop dz-multi-pop" id=(pop_id) tabindex="-1" role="group" aria-label=(m.label) x-ref="pop" x-show="open" xa-bottom-start-4="$refs.trigger" style="display:none" {
                div class="dz-multi-field" {
                    (icon(Icon::Search, "dz-lead"))
                    span class="dz-multi-chips" x-ref="chips" {}
                    input class="dz-input" x-ref="search" type="text" role="combobox" aria-autocomplete="list" aria-haspopup="listbox" aria-expanded="true"
                        aria-controls=(list_id) aria-label=(format!("Search {}", m.label.to_lowercase())) autocomplete="off" spellcheck="false" placeholder=(format!("Search {lower}")) x-on:input="filter";
                }
                div class="dz-multi-actions" {
                    span class="dz-multi-n" role="status" aria-live="polite" x-ref="status" { (format!("{n} of {} selected", all.len())) }
                    button type="button" class="dz-link" data-all { "Select all" }
                    button type="button" class="dz-link" data-none { "Clear" }
                }
                div class="dz-multi-list" role="listbox" aria-multiselectable="true" id=(list_id) aria-label=(m.label) x-ref="list" {
                    @for (i, c) in all.iter().enumerate() {
                        div class="dz-opt dz-multi-opt" role="option" id=(format!("{}-o{i}", m.id)) data-i=(i) data-value=(c.value) data-label=(c.label) title=(c.label)
                            aria-selected=(if on(c) { "true" } else { "false" }) {
                            span class="dz-box" { (icon(Icon::Check, "dz-check")) }
                            span class="dz-opt-t" { span class="dz-opt-label" { (c.label) } }
                            @if let Some(k) = c.count { span class="dz-count" { (k) } }
                        }
                    }
                    div class="dz-empty" role="presentation" hidden { "No match" }
                }
            }
        }
    }
}
