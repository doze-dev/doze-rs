//! The edit dialog of a dynamic broker config (board `broker-config-edit`): which config, for which brokers, the new value,
//! what Kafka says about it, and the call beside it. One POST form; Save sends IncrementalAlterConfigs on the BROKER
//! resource, Check sends the same call with `validate_only` and changes nothing.
//!
//! The dialog is an overlay over the Settings page: `edit_overlay` is the pure overlay body (the shell of `ui/dialog.rs`
//! until the overlay system lands), `with_overlay` puts it on its parent for a direct visit.

use maud::{Markup, html};

use super::settings_url;
use crate::base::Base;
use crate::model::{SettingsEditView, SettingsLine, SettingsScope};
use crate::settings_words::blurb;
use crate::ui::dialog::{cancel, modal};
use crate::ui::glyph::Glyph;
use crate::ui::glyph_extra::{Extra, glyph_extra};
use crate::ui::notice::{Tone, callout};
use crate::ui::write::{apicall, field_error, spans, text_input};

fn teach_line(label: &str, l: &SettingsLine) -> Markup {
    html! {
        @if !l.text.is_empty() {
            div class="teach" { b { (label) ":" } " " (spans(&l.text)) @if !l.dim.is_empty() { " " span class="dim" { (l.dim) } } }
        }
    }
}

fn select(v: &SettingsEditView) -> Markup {
    html! {
        div class="field" {
            label for="key" { "Config" }
            div class="input sel mono" {
                select id="key" name="key" {
                    @for k in &v.keys { option value=(k) selected[*k == v.key] { (k) } }
                }
                (glyph_extra(Extra::ChevD, 14))
            }
            div class="help" {
                "One of the five. Another, such as " span class="mono" { "num.partitions" }
                ", cannot be changed while the broker runs: "
                button class="lnk" type="submit" name="probe" value="num.partitions" { "what Kafka says" }
                ". To move to another of the five, choose it and press Check."
            }
        }
    }
}

fn scope(v: &SettingsEditView) -> Markup {
    let on = |s: SettingsScope| v.scope == s;
    html! {
        div class="field" {
            span class="lab" id="scope-label" { "Applies to" }
            div class="seg" role="radiogroup" aria-labelledby="scope-label" style="align-self:flex-start" {
                label { input type="radio" name="scope" value="all" checked[on(SettingsScope::All)]; "All brokers" }
                label { input type="radio" name="scope" value="this" checked[on(SettingsScope::This)]; "This broker (1)" }
            }
            div class="help" { "This broker's own value wins over the one for all brokers." }
        }
    }
}

fn checked(v: &SettingsEditView) -> Markup {
    html! {
        @if let Some(c) = &v.checked {
            (callout(Tone::Plain, Glyph::Check, html! {
                b { "Kafka would take this." }
                " The same call with validate_only answered 0, and nothing was changed."
                dl class="kv sm" style="margin-top:12px" {
                    dt { (v.key) }
                    dd class="mono" { (c.before) " " span class="dim" { "to" } " " (c.after) }
                }
            }))
        }
    }
}

fn value(v: &SettingsEditView) -> Markup {
    html! {
        div class="field" {
            label for="value" { "New value" }
            (text_input("value", "value", &v.value, true, v.unit.as_deref(), v.error.is_some()))
            @if let Some(e) = &v.error { (field_error(e)) }
            div class="stack-y" style="gap:10px" data-extra="teaching" {
                (teach_line("Now", &v.teach.now))
                (teach_line("Allowed", &v.teach.allowed))
                (teach_line("Takes effect", &v.teach.takes_effect))
                (teach_line("Who sees it", &v.teach.who_sees))
            }
        }
    }
}

fn body(v: &SettingsEditView) -> Markup {
    html! {
        p style="margin:0 0 20px" { (blurb(&v.key)) }
        div class="form" style="gap:22px" {
            input type="hidden" name="was" value=(v.key);
            (select(v))
            (scope(v))
            (value(v))
            (checked(v))
            (apicall("The call", &v.call, None))
        }
    }
}

/// The title of the dialog.
#[must_use]
pub fn edit_title(v: &SettingsEditView) -> String {
    format!("Edit {}", v.key)
}

/// The dialog, as an overlay body.
#[must_use]
pub fn edit_dialog(v: &SettingsEditView, base: &Base, inline: bool) -> Markup {
    modal(
        "broker-config-edit",
        &base.url("/settings/config/edit"),
        inline,
        html! { (edit_title(v)) },
        body(v),
        html! {
            (cancel(&settings_url(base)))
            button class="btn" type="submit" name="check" value="1" { "Check" }
            button class="btn pri" type="submit" data-op="IncrementalAlterConfigs" { "Save" }
        },
    )
}

/// The overlay the dialog boards are: only the dialog, for the overlay system to draw over the Settings page.
#[must_use]
pub fn edit_overlay(v: &SettingsEditView, base: &Base, inline: bool) -> Markup {
    edit_dialog(v, base, inline)
}

/// The Settings page with an overlay open on it: the parent's content, then the overlay.
#[must_use]
pub fn with_overlay(parent: Markup, overlay: Markup) -> Markup {
    html! { (parent) (overlay) }
}
