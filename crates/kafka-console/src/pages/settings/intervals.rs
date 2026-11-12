//! The Intervals section: how often the broker looks at things on its own. Read only: set when the broker starts.

use maud::{Markup, html};

use crate::model::SettingsIntervals;

fn rows(i: &SettingsIntervals) -> Markup {
    html! {
        @for r in &i.rows {
            tr {
                td class="name" { (r.name) span class="sub" { (r.what) } }
                td class="mono" { (r.value) }
                td class="mono" { (r.flag) }
                td class="mono" { (r.default) }
                td class="dim" { (r.note) }
            }
        }
    }
}

/// The section.
#[must_use]
pub(super) fn intervals_section(i: &SettingsIntervals) -> Markup {
    html! {
        div class="sec" id="intervals" {
            h2 { "Intervals" }
            p class="lede" {
                "How often the broker looks at things on its own. " b { "Read only: set when the broker starts." }
                @if !i.retention_every.is_empty() {
                    " Retention is applied every " b { (i.retention_every) } ". "
                    @if i.cleaner_on {
                        "The cleaner looks every " b { (i.cleaner_every) } "."
                    } @else {
                        "The cleaner is off (" span class="mono" { "--log-cleaner-enable=false" } ")."
                    }
                }
            }
            @if i.rows.is_empty() {
                p class="help" { "The broker did not say how it was started." }
            } @else {
                table class="t" {
                    thead { tr {
                        th scope="col" { "Interval" } th scope="col" { "Value" } th scope="col" { "Flag" }
                        th scope="col" { "Default" } th scope="col" { "What it does" }
                    } }
                    tbody { (rows(i)) }
                }
                p class="help" style="margin:14px 0 0" {
                    "A transaction past its timeout is aborted by a check every " span class="mono" { (i.transactions) }
                    ", and its producer is fenced. That check has no flag, and the timeout belongs to each producer. A running broker cannot change these: Kafka refuses with \"Cannot update these configs dynamically\", and so does this one. To change an interval, stop the broker and start it with the flag."
                }
            }
        }
    }
}
