//! A row of the Config table and the two things that open under it: where its value comes from, and Kafka's text.

use maud::{Markup, html};

use super::common::{edit_url, plain_value, reset_url, source_tag, value_cell};
use super::links::{toggled, token, url};
use crate::base::Base;
use crate::config_effects::sentence_of;
use crate::config_words::{first_sentence, has_more, plain_text, type_word};
use crate::model::{ConfigAsk, ConfigPace, ConfigRow, ConfigSource, ConfigSynonym};
use crate::ui::button::{Size, Variant, class};
use crate::ui::client::dialog_link;
use crate::ui::glyph::{Glyph, glyph};

/// What a row needs besides itself.
#[derive(Clone, Copy)]
pub(super) struct Ctx<'a> {
    pub base: &'a Base,
    pub topic: &'a str,
    pub ask: &'a ConfigAsk,
    pub pace: Option<ConfigPace>,
}

fn opened(c: &Ctx<'_>, kind: &str, key: &str) -> bool {
    c.ask.open.contains(&token(kind, key))
}

/// A ghost button that opens or shuts one of the two panels: a link that carries the state of the page.
fn expander(c: &Ctx<'_>, kind: &str, key: &str, label: &str) -> Markup {
    let open = opened(c, kind, key);
    let next = toggled(c.ask, &token(kind, key));
    let href = url(c.base, c.topic, &next, Some(key));
    html! {
        a class=(class(Variant::Plain, Size::Small) + " ghost") href=(href) aria-expanded=(open) {
            (glyph(Glyph::ChevR, 14, ""))
            (label)
        }
    }
}

fn first_cell(r: &ConfigRow, c: &Ctx<'_>) -> Markup {
    html! {
        td {
            span class="mono bright" { (r.name) }
            @if !r.doc.is_empty() { span class="sub" { (first_sentence(&r.doc)) } }
            @if let Some(s) = sentence_of(&r.name, c.pace) { span class="sub" { (s) } }
        }
    }
}

fn source_cell(r: &ConfigRow, c: &Ctx<'_>) -> Markup {
    html! {
        td {
            (source_tag(r.source))
            @if r.changed() { span class="sub" { (expander(c, "where", &r.name, "Where from")) } }
        }
    }
}

fn actions_cell(r: &ConfigRow, c: &Ctx<'_>) -> Markup {
    html! {
        td {
            div class="row" style="justify-content:flex-end;gap:6px" {
                @if r.on_topic() {
                    (dialog_link(&(class(Variant::Plain, Size::Small) + " ghost"), &reset_url(c.base, c.topic, &r.name), html! { "Reset" }))
                }
                @if has_more(&r.doc) { (expander(c, "text", &r.name, "Kafka's text")) }
                (dialog_link(&class(Variant::Plain, Size::Small), &edit_url(c.base, c.topic, &r.name), html! { "Edit" }))
            }
        }
    }
}

/// The row, and under it what has been opened.
pub(super) fn row(r: &ConfigRow, c: &Ctx<'_>) -> Markup {
    html! {
        tr id=(format!("c-{}", r.name)) {
            (first_cell(r, c)) (value_cell(r)) (source_cell(r, c)) (actions_cell(r, c))
        }
        @if opened(c, "where", &r.name) { (where_from(r)) }
        @if opened(c, "text", &r.name) { (kafkas_text(r)) }
    }
}

/// The places a value can come from, in the order the broker looks, with the one that applies marked.
const PLACES: [(ConfigSource, &str, &str); 5] = [
    (ConfigSource::DynamicTopic, "Dynamic topic", "on this topic"),
    (
        ConfigSource::DynamicBroker,
        "Dynamic broker, this broker",
        "set for this broker",
    ),
    (
        ConfigSource::DynamicDefault,
        "Dynamic broker, all brokers",
        "set for all brokers",
    ),
    (
        ConfigSource::StaticFlag,
        "Static flag",
        "a start flag that is not Kafka's default",
    ),
    (ConfigSource::Default, "Kafka's default", "Kafka's own"),
];

fn place_row(r: &ConfigRow, place: &(ConfigSource, &str, &str), wins: bool) -> Markup {
    let (source, title, detail) = *place;
    let syn: Option<&ConfigSynonym> = r.synonyms.iter().find(|s| s.source == source);
    let behind = r
        .synonyms
        .iter()
        .find(|s| s.source != ConfigSource::DynamicTopic)
        .map(|s| s.name.as_str());
    let name = syn.map(|s| s.name.as_str()).or(behind).unwrap_or(&r.name);
    html! {
        div class={"r " @if wins { "ok" } @else { "no" }} style="text-align:left" {
            (glyph(if wins { Glyph::Check } else { Glyph::X }, 16, ""))
            div {
                (title)
                span class="sub" {
                    @if source == ConfigSource::DynamicTopic { (r.name) " " (detail) }
                    @else if syn.is_some() { (name) ": " (detail) }
                    @else { (name) ": not set" }
                }
            }
            span class="v" { (syn.map_or("none", |s| s.value.as_str())) }
        }
    }
}

fn takes_over(r: &ConfigRow) -> Option<String> {
    let next = r.fallback().filter(|_| r.on_topic())?;
    let who = match next.source {
        ConfigSource::StaticFlag => "the flag",
        ConfigSource::Default => "Kafka's default",
        _ => "the broker's value",
    };
    Some(format!(
        "Reset the topic value and {who}, {}, takes over.",
        next.value
    ))
}

fn where_from(r: &ConfigRow) -> Markup {
    let winner = PLACES
        .iter()
        .position(|p| r.synonyms.iter().any(|s| s.source == p.0))
        .unwrap_or(PLACES.len());
    html! {
        tr class="sel" {
            td colspan="4" {
                b { "Where " (r.name) " comes from." }
                " The first place with a value wins. DescribeConfigs lists them when synonyms are asked for."
                div class="rules" style="margin-top:12px" {
                    @for (i, p) in PLACES.iter().enumerate() { (place_row(r, p, i == winner)) }
                }
                @if let Some(t) = takes_over(r) { p class="help" style="margin:12px 0 0" { (t) } }
            }
        }
    }
}

fn kafkas_text(r: &ConfigRow) -> Markup {
    let default = r.kafka_default().map_or("none", |d| d.value.as_str());
    html! {
        tr class="sel" {
            td colspan="4" {
                b { "Kafka's text for " (r.name) }
                ", as DescribeConfigs returns it when documentation is asked for."
                div class="teach" style="max-width:none;margin-top:12px" {
                    div style="font-size:13px;line-height:21px" { (plain_text(&r.doc)) }
                }
                dl class="kv sm" style="margin-top:12px" {
                    dt { "Type" } dd class="mono" { (type_word(r.kind)) }
                    dt { "Allowed" } dd { (r.allowed) }
                    dt { "Default" } dd class="mono" { (default) }
                    dt { "Here" } dd { (plain_value(&r.name, &r.value)) span class="sub" { (r.source.label()) } }
                }
            }
        }
    }
}
