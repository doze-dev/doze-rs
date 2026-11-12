//! The two answers of the edit dialog: saved (board `broker-config-saved`) and refused (board `broker-config-refused`).
//! The code and the sentence of a refusal are the broker's own; "what would pass" and the other refusals are what it said
//! to dry runs asked just now (`validate_only`: nothing was changed by any of them).

use maud::{Markup, html};

use super::configs::saved_table;
use super::page::sub;
use super::settings_url;
use crate::base::Base;
use crate::format::number;
use crate::model::{SettingsRefused, SettingsSaved, SettingsScope};
use crate::ui::code::block;
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::glyph_groups::{GroupGlyph, group_glyph};
use crate::ui::layout::{card, page_header};
use crate::ui::write::{apicall, spans};

fn back(base: &Base) -> Markup {
    html! {
        a class="btn" href=(settings_url(base)) { (group_glyph(GroupGlyph::Back, 16, "")) "Back to Settings" }
    }
}

fn banner(s: &SettingsSaved) -> Markup {
    let shown = s
        .value
        .trim()
        .parse::<i64>()
        .map_or_else(|_| s.value.clone(), number);
    html! {
        div class="banner" {
            (glyph(Glyph::Check, 18, ""))
            div {
                b { "Saved." } " " span class="mono" { (s.key) } " is now " span class="mono" { (shown) }
                @if let Some(g) = &s.gloss { " (" (g) ")" }
                @match s.scope { SettingsScope::All => " for all brokers.", SettingsScope::This => " for this broker." }
                " " (s.effect) "."
            }
        }
    }
}

/// The saved state: the Settings page's title, a banner, the five configs as they are now, who sees the change, the call.
#[must_use]
pub fn settings_saved(s: &SettingsSaved, base: &Base) -> Markup {
    html! {
        (page_header("Settings", sub(), html! {}))
        (banner(s))
        (saved_table(&s.rows, s.set, base))
        div class="g2" {
            div class="card" data-extra="live-sentence" { h2 { "Who sees it" } p class="sent" style="margin:8px 0 0" { (spans(&s.who_sees)) } }
            (apicall("The call that was made", &s.call, None))
        }
    }
}

fn callout(r: &SettingsRefused) -> Markup {
    html! {
        div class="callout refused" data-extra="refusal" style="margin-bottom:28px" {
            (glyph(Glyph::Warn, 18, ""))
            div {
                span class="rw refused" { "Refused " span class="mono" { (r.refusal.name) " (" (r.refusal.code) ")" } }
                div style="margin-top:6px;font-size:15px" class="mono bright" { (r.refusal.message) }
                div style="margin-top:8px" { (spans(&r.rule)) }
            }
        }
    }
}

fn would_pass(r: &SettingsRefused, base: &Base) -> Markup {
    card(html! {
        h2 { "What would pass" }
        p class="sent" style="margin:8px 0 14px" { (spans(&r.would_pass)) }
        div class="row wrap" {
            @if let Some(f) = &r.flag { span class="pill mono" { (f) } }
            a class="lnk" href={(settings_url(base)) "#flags"} { "All flags" }
        }
    })
}

fn others(r: &SettingsRefused) -> Markup {
    card(html! {
        h2 { "The other ways a change is refused" }
        p class="lede" style="margin-bottom:12px" { "Each is asked of the broker now, with Kafka's own text; nothing is changed by asking." }
        div class="rules" {
            @for (title, said, sub) in &r.others {
                div class="r no" {
                    span class="gl" style="color:var(--refused)" { (glyph(Glyph::Warn, 16, "")) }
                    div {
                        (title)
                        span class="sub mono" style="font-size:12.5px;color:var(--text)" { (said) }
                        span class="sub" { (sub) }
                    }
                }
            }
        }
    })
}

fn fixed(r: &SettingsRefused) -> Markup {
    card(html! {
        h2 { "Fixed while it runs" }
        p class="lede" style="margin-bottom:12px" { "Kafka configs this broker holds but does not change at run time." }
        div class="row wrap" style="gap:8px" { @for k in &r.fixed { span class="tag" { (k) } } }
    })
}

/// The refused state: Kafka's words first, then what would pass and the other refusals beside the call and the fixed configs.
#[must_use]
pub fn settings_refused(r: &SettingsRefused, base: &Base) -> Markup {
    let sub = html! {
        p class="sub" {
            @if r.validate_only {
                "Asked as a dry run (" span class="mono" { "validate_only" } "): nothing was changed. "
            }
            "A tool asked to change a config that cannot change while the broker runs, or a value Kafka will not take. Kafka says no, and so does this broker, in the same words. "
            b { "The console never offers it" } "; this is what " span class="mono" { "kafka-configs.sh" } " or another client would see."
        }
    };
    html! {
        div class="page-h" {
            div { div class="title-row" { h1 { "Refused" } span class="st refused" { "Refused" } } (sub) }
            div class="acts" { (back(base)) }
        }
        (callout(r))
        div class="split wide" {
            div class="stack-y" { (would_pass(r, base)) @if !r.others.is_empty() { (others(r)) } }
            div class="stack-y" {
                div class="apicall" data-extra="api-preview" {
                    div class="hd" { (glyph(Glyph::Code, 14, "")) "The call that was refused" }
                    (block(&r.call, None, Some("margin:0")))
                }
                @if !r.fixed.is_empty() { (fixed(r)) }
            }
        }
    }
}
