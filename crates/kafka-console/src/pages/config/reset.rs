//! Reset to default (board `config-reset`): IncrementalAlterConfigs DELETE of the value the topic sets.
//!
//! The board's "Segments 1 to 7 keep their size" and "the active segment (0.6 MiB)" need the files of the log, which
//! the dialog does not read; the teaching says when the change counts and what stays, as the broker does it.

use maud::{Markup, html};

use super::common::{config_url, plain_value};
use crate::base::Base;
use crate::config_effects::{moment, takes_effect};
use crate::model::{ConfigResetView, ConfigValue};
use crate::ui::dialog::{cancel, modal};
use crate::ui::write::{apicall, refused};

fn after(key: &str, v: Option<&ConfigValue>) -> Markup {
    html! {
        @if let Some(v) = v {
            (plain_value(key, &v.value)) span class="sub" { (v.source.label()) }
        } @else {
            span class="mono" { "none" } span class="sub" { "the broker lists no value for it" }
        }
    }
}

fn body(v: &ConfigResetView) -> Markup {
    let when = moment(&v.row.name).map(|m| takes_effect(m, v.pace));
    html! {
        input type="hidden" name="key" value=(v.row.name);
        p style="margin:0 0 16px" {
            "The value set on " span class="mono" { (v.topic) } " is removed, so the topic follows the broker again."
        }
        dl class="kv sm" {
            dt { "Now" } dd { (plain_value(&v.row.name, &v.row.value)) span class="sub" { (v.row.source.label()) } }
            dt { "After" } dd { (after(&v.row.name, v.after.as_ref())) }
        }
        @if let Some(w) = when {
            div class="teach" data-extra="teaching" style="margin-top:16px" {
                div { b { "Takes effect" } " " (w) }
            }
        }
        @if let Some(r) = &v.refusal { div style="margin-top:16px" { (refused(r)) } }
        div style="margin-top:16px" { (apicall("The call", &v.call, None)) }
    }
}

/// The dialog.
#[must_use]
pub fn reset_dialog(v: &ConfigResetView, base: &Base, inline: bool) -> Markup {
    modal(
        "config-reset",
        &config_url(base, &v.topic, "/reset"),
        inline,
        html! { "Reset " (v.row.name) " to default?" },
        body(v),
        html! {
            (cancel(&config_url(base, &v.topic, "")))
            button class="btn danger fill" type="submit" data-op="IncrementalAlterConfigs" { "Reset to default" }
        },
    )
}
