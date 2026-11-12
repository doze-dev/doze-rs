//! The Version profile section: the Kafka the broker stands for and the APIs ApiVersions offers, the first seven in view and
//! all of them behind "Show all".

use maud::{Markup, html};

use crate::model::{SettingsApi, SettingsProfile};
use crate::ui::glyph_extra::{Extra, glyph_extra};

/// The APIs shown before "Show all".
const FIRST: usize = 7;

fn rows(apis: &[SettingsApi]) -> Markup {
    html! {
        @for a in apis {
            tr { td class="mono name" { (a.name) } td class="mono" { (a.versions) } td class="dim" { (a.family) } }
        }
    }
}

fn colgroup() -> Markup {
    html! { colgroup { col style="width:32%"; col style="width:20%"; col; } }
}

fn first_table(apis: &[SettingsApi]) -> Markup {
    html! {
        table class="t" style="table-layout:fixed" {
            (colgroup())
            thead { tr { th scope="col" { "API" } th scope="col" { "Versions" } th scope="col" { "Family" } } }
            tbody { (rows(apis)) }
        }
    }
}

fn rest_table(apis: &[SettingsApi]) -> Markup {
    html! {
        details style="margin-top:12px" {
            summary class="btn sm" style="list-style:none;cursor:pointer" {
                (glyph_extra(Extra::ChevD, 14)) "Show all " (apis.len() + FIRST) " APIs"
            }
            table class="t" style="table-layout:fixed;margin-top:12px" aria-label="Every API offered, continued" {
                (colgroup())
                tbody { (rows(apis)) }
            }
        }
    }
}

fn names(apis: &[SettingsApi]) -> Markup {
    html! {
        @for (i, a) in apis.iter().enumerate() {
            @if i > 0 { @if i + 1 == apis.len() { " and " } @else { ", " } }
            (a.name) " (" (a.versions.replace(" only", "")) ")"
        }
    }
}

fn notes(p: &SettingsProfile) -> Markup {
    html! {
        p class="help" style="margin:16px 0 0" {
            @if p.major == 4 {
                "Produce starts at v3 and Fetch at v4, the first versions that carry record batch format v2, the only format the log stores. OffsetCommit and OffsetFetch stop at v9: from v10 the wire form uses topic ids, which this broker does not use. Nine APIs lose their oldest versions at Kafka 4 (KIP-896); the other profiles, 1 to 3, are chosen at start with "
            } @else {
                "Produce starts at v3 and Fetch at v4, the first versions that carry record batch format v2, the only format the log stores. The other profiles, 1 to 4, are chosen at start with "
            }
            span class="mono" { "--kafka-version" } ". "
            @if !p.with_sasl.is_empty() {
                b { "Offered only with SASL on:" } " " (names(&p.with_sasl)) ", "
                @if p.sasl_on { "which are in the list above: this broker has SASL on. " }
                @else { "which would make " (p.apis.len() + p.with_sasl.len()) "; this broker has SASL off. " }
            }
            @if !p.not_offered.is_empty() {
                b { "Not offered:" } " " (p.not_offered.join(", ")) " (no handler here, or switched off)."
            }
        }
    }
}

/// The section.
#[must_use]
pub(super) fn profile_section(p: &SettingsProfile, name: &str) -> Markup {
    let (first, rest) = p.apis.split_at(p.apis.len().min(FIRST));
    html! {
        div class="sec" id="version-profile" {
            h2 { "Version profile" }
            p class="lede" {
                "The Kafka the broker stands for, and the APIs a client is offered: "
                b class="mono" { "--kafka-version " (p.major) } ", " b { (p.apis.len()) " APIs" }
                ", each with the versions a real " (name) " broker offers, never more. It is what ApiVersions answers. A client picks, for each API, the highest version both sides have."
            }
            (first_table(first))
            @if !rest.is_empty() { (rest_table(rest)) }
            (notes(p))
        }
    }
}
