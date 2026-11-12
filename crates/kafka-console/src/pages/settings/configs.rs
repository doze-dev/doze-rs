//! The Broker configs section: the five dynamic configs, each with its value, where the value comes from, when a change
//! counts, and the Edit button that opens its dialog.

use maud::{Markup, html};

use super::edit_url;
use crate::base::Base;
use crate::gather::settings_configs::source_label;
use crate::model::{SettingsConfigRow, SettingsConfigs};
use crate::ui::client::dialog_link;
use crate::ui::glyph_settings::{SettingsGlyph, settings_glyph};

fn row(r: &SettingsConfigRow, base: &Base) -> Markup {
    html! {
        tr {
            td class="name" { span class="mono" { (r.name) } span class="sub" { (r.what) } }
            td class="mono" {
                (r.value)
                @if let Some(g) = &r.gloss { span class="sub" style="font-family:Inter,sans-serif" { (g) } }
            }
            td { span class="pill" { (source_label(r.source)) } span class="sub" { (r.source_note) } }
            td { (r.takes_effect) }
            td class="r" {
                (dialog_link("btn sm", &edit_url(base, &r.name), html! { (settings_glyph(SettingsGlyph::Edit, 14)) "Edit" }))
            }
        }
    }
}

fn table(rows: &[SettingsConfigRow], base: &Base) -> Markup {
    html! {
        table class="t" {
            thead { tr {
                th scope="col" { "Config" } th scope="col" { "Value" } th scope="col" { "Source" }
                th scope="col" { "Takes effect" } th scope="col" aria-label="Edit" {}
            } }
            tbody { @for r in rows { (row(r, base)) } }
        }
    }
}

/// The Broker configs section of the page.
#[must_use]
pub(super) fn configs_section(c: &SettingsConfigs, base: &Base) -> Markup {
    html! {
        div class="sec" id="broker-configs" {
            h2 { "Broker configs" }
            p class="lede" {
                "Source says where the value in force comes from: a dynamic setting first, then the flag it started with, then Kafka's own default. A topic that sets its own value keeps it."
            }
            (table(&c.rows, base))
            div class="help" style="margin:14px 0 0" data-extra="teaching" {
                "A change is kept on disk and is still there after a restart. It applies to all brokers or to this one (there is one). These are the only five: another config is refused "
                (refused_link(base, c.node_id))
                "."
            }
        }
    }
}

/// "in Kafka's words": a form that asks Kafka for a dry run of a config that cannot change, and shows its answer.
fn refused_link(base: &Base, _node: i32) -> Markup {
    html! {
        form method="post" action=(base.url("/settings/config/edit")) class="inline" {
            button class="lnk" type="submit" name="probe" value="num.partitions" { "in Kafka's words" }
        }
    }
}

/// The table of the saved page: the same rows under the heading the board gives it.
#[must_use]
pub(super) fn saved_table(rows: &[SettingsConfigRow], set: usize, base: &Base) -> Markup {
    html! {
        div class="sec" {
            h2 { "Dynamic broker configs" }
            p class="lede" {
                (set) " of the 5 " @if set == 1 { "is" } @else { "are" }
                " now set at run time. " @if set == 1 { "It is" } @else { "They are" } " kept on disk and "
                @if set == 1 { "is" } @else { "are" } " there after a restart."
            }
            (table(rows, base))
        }
    }
}
